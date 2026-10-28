//! Mounted assistants expose the existing Client projection, never a second history.
use super::*;
use crate::plugins::host::sdk::{public_output, values::encode};
use futures::StreamExt;
use gpui_shell::{HostError, HostValue};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct Observations(watch::Sender<BTreeMap<String, Observation>>);

#[derive(Clone)]
struct Observation {
    history: sailry_client::conversation::View,
    client: std::sync::Arc<sailry_client::Client>,
    prefix: String,
    runtime: tokio::runtime::Handle,
    details: sailry_client::conversation::details::Tracker,
}

impl Default for Observations {
    fn default() -> Self {
        Self(watch::channel(BTreeMap::new()).0)
    }
}

impl Observations {
    pub fn remove(&self, id: &str) {
        self.0
            .send_if_modified(|values| values.remove(id).is_some());
    }

    pub fn attach(&self, id: String, package: &str, chat: &Entity<Chat>, cx: &mut App) {
        let prefix = format!(
            "plugin_{}_",
            &blake3::hash(package.as_bytes()).to_hex()[..16]
        );
        let sender = self.0.clone();
        let publish = move |chat: &Entity<Chat>, cx: &App| {
            let chat = chat.read(cx);
            let history = chat.tool_history();
            sender.send_if_modified(|values| {
                if values.get(&id).is_some_and(|previous| {
                    previous.history.connected == history.connected
                        && std::sync::Arc::ptr_eq(&previous.history.calls, &history.calls)
                        && previous
                            .history
                            .snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.sequence)
                            == history.snapshot.as_ref().map(|snapshot| snapshot.sequence)
                }) {
                    false
                } else {
                    let binding = chat.binding();
                    let details = values
                        .get(&id)
                        .filter(|previous| {
                            std::sync::Arc::ptr_eq(&previous.client, &binding.client)
                        })
                        .map(|previous| previous.details.clone())
                        .unwrap_or_else(|| {
                            sailry_client::conversation::details::Tracker::new(
                                binding.client.clone(),
                                binding.runtime.handle().clone(),
                            )
                        });
                    values.insert(
                        id.clone(),
                        Observation {
                            history: history.clone(),
                            client: binding.client,
                            prefix: prefix.clone(),
                            runtime: binding.runtime.handle().clone(),
                            details,
                        },
                    );
                    true
                }
            });
        };
        publish(chat, cx);
        cx.observe(chat, move |chat, cx| publish(&chat, cx))
            .detach();
    }

    pub fn module(&self, module: HostModule, stop: CancellationToken) -> HostModule {
        let receiver = self.0.subscribe();
        module.async_function("nextAssistantTools", move |args| {
            let seen = args
                .get(0)
                .and_then(HostValue::as_str)
                .unwrap_or("")
                .to_owned();
            let receiver = receiver.clone();
            let stop = stop.clone();
            Ok(async move { encode(next(receiver, &seen, stop).await?) })
        })
    }
}

async fn next(
    mut receiver: watch::Receiver<BTreeMap<String, Observation>>,
    seen: &str,
    stop: CancellationToken,
) -> Result<Value, HostError> {
    loop {
        if stop.is_cancelled() {
            return Err(HostError::new("plugin view is closed"));
        }
        let observed = receiver.borrow_and_update().clone();
        let mut details = observed
            .values()
            .map(|observation| observation.details.subscribe())
            .collect::<Vec<_>>();
        let cursor = cursor(&observed)?;
        if cursor != seen {
            let mut value = BTreeMap::new();
            for (id, observed) in observed {
                value.insert(id, read(observed, stop.clone()).await?);
            }
            return Ok(json!({"cursor":cursor,"conversations":value}));
        }
        let changed = async {
            if details.is_empty() {
                return std::future::pending::<()>().await;
            }
            let _ = futures::future::select_all(
                details
                    .iter_mut()
                    .map(|details| Box::pin(details.changed())),
            )
            .await;
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
            _ = changed => {},
            changed = receiver.changed() => if changed.is_err() {
                return Err(HostError::new("assistant subscription ended"));
            },
        }
    }
}

type Selected<'a> = (
    &'a sailry_client::conversation::tools::Call,
    &'a str,
    sailry_protocol::tool::detail::Reference,
    i64,
    Option<u64>,
);

fn selected(observation: &Observation) -> Vec<Selected<'_>> {
    let Some(snapshot) = &observation.history.snapshot else {
        return Vec::new();
    };
    let page = &snapshot.page;
    observation
        .history
        .calls
        .iter()
        .filter_map(|call| {
            let name = call.name.strip_prefix(&observation.prefix)?;
            let reference = call.detail_reference(page)?;
            let source = page
                .entries
                .iter()
                .find(|entry| entry.id == call.source.entry)?;
            let sequence = call
                .response
                .as_ref()
                .and_then(|response| page.entries.iter().find(|entry| entry.id == response.entry))
                .map(|entry| entry.sequence);
            Some((call, name, reference, source.timestamp_ms, sequence))
        })
        .collect()
}

fn cursor(observed: &BTreeMap<String, Observation>) -> Result<String, HostError> {
    let source = observed
        .iter()
        .map(|(id, observation)| {
            let page = observation.history.snapshot.as_ref().map(|snapshot| {
                (
                    snapshot.page.session,
                    snapshot.page.revision,
                    snapshot
                        .page
                        .entries
                        .iter()
                        .map(|entry| entry.sequence)
                        .max(),
                )
            });
            let calls = selected(observation)
                .into_iter()
                .map(|(call, name, reference, timestamp, sequence)| {
                    (
                        format!("{}:{}", call.turn, call.source.key()),
                        name,
                        reference,
                        call.state,
                        timestamp,
                        sequence,
                    )
                })
                .collect::<Vec<_>>();
            let details = *observation.details.subscribe().borrow();
            (id, observation.history.connected, page, calls, details)
        })
        .collect::<Vec<_>>();
    Ok(blake3::hash(
        &serde_json::to_vec(&source).map_err(|error| HostError::new(error.to_string()))?,
    )
    .to_hex()
    .to_string())
}

async fn read(observation: Observation, stop: CancellationToken) -> Result<Value, HostError> {
    let runtime = observation.runtime.clone();
    let mut jobs = tokio::task::JoinSet::new();
    jobs.spawn_on(async move { snapshot(&observation).await }, &runtime);
    tokio::select! {
        _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
        value = jobs.join_next() => value.expect("snapshot task").map_err(|error| HostError::new(error.to_string()))?,
    }
}

async fn snapshot(observation: &Observation) -> Result<Value, HostError> {
    use sailry_client::conversation::details::{Read, State};
    let history = &observation.history;
    let Some(snapshot) = &history.snapshot else {
        observation.details.set([]);
        return Ok(json!({"connected":history.connected,"calls":[]}));
    };
    let page = &snapshot.page;
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    // Explicit reads share the Client budget; only failed references stay in the
    // Tracker for its bounded retries and capacity notifications.
    let selected = selected(observation)
        .into_iter()
        .map(|(call, name, reference, timestamp, sequence)| {
            (
                format!("{}:{}", call.turn, call.source.key()),
                name.to_owned(),
                reference,
                call.state,
                timestamp,
                sequence,
            )
        })
        .collect::<Vec<_>>();
    observation.details.retain(|reference| {
        selected
            .iter()
            .any(|(_, _, current, ..)| current == reference)
    });
    let mut details = futures::stream::iter(selected)
        .map(
            |(key, name, reference, state, timestamp, sequence)| async move {
                let result = match observation.details.state(&reference) {
                    State::Ready(detail) => Some(Ok(Read::Ready(detail))),
                    State::Loading
                    | State::Failed {
                        retry_at: Some(_), ..
                    } => None,
                    State::Failed { fault, .. } => Some(Err(fault)),
                    State::TooLarge(limit) => Some(Ok(Read::TooLarge(limit))),
                    State::Superseded => {
                        let result = observation.client.detail(reference.clone()).await;
                        if result.is_err() {
                            observation.details.include(reference.clone());
                        }
                        Some(result)
                    }
                };
                ((key, name, reference, state, timestamp, sequence), result)
            },
        )
        .buffered(4);
    while let Some(((key, name, reference, state, timestamp_ms, sequence), result)) =
        details.next().await
    {
        let Some(result) = result else {
            continue;
        };
        let detail = match result {
            Ok(Read::Ready(detail)) => detail,
            Err(fault) => {
                errors.push(json!({"key":key,"message":fault.message}));
                continue;
            }
            Ok(Read::TooLarge(limit)) => {
                errors.push(json!({"key":key,"message":limit.message()}));
                continue;
            }
            Ok(Read::Superseded) => continue,
        };
        let result = detail.result.as_ref().map(|value| {
            serde_json::from_value(value.clone())
                .ok()
                .and_then(|output| public_output(output).ok())
                .unwrap_or_else(|| value.clone())
        });
        calls.push(json!({"key":key,"name":name,"arguments":detail.arguments,
            "state":state,"timestamp_ms":timestamp_ms,"result":result,"sequence":sequence.map(|sequence|sequence.to_string())}));
        observation.details.remove(&reference);
    }
    Ok(
        json!({"connected":history.connected,"session":page.session,"revision":page.revision.to_string(),
        "latest":page.entries.iter().map(|entry|entry.sequence).max().map(|value|value.to_string()),"calls":calls,"errors":errors}),
    )
}

#[cfg(test)]
mod tests;
