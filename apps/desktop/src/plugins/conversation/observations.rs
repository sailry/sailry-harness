//! Mounted assistants expose the existing Client projection, never a second history.
use super::*;
use crate::plugins::host::sdk::{public_output, values::encode};
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
    details: std::sync::Arc<sailry_client::conversation::details::Tracker>,
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
                    let client = binding.client;
                    let details = values
                        .get(&id)
                        .filter(|previous| {
                            previous.client.target() == client.target() && previous.prefix == prefix
                        })
                        .map(|previous| previous.details.clone())
                        .unwrap_or_else(|| {
                            std::sync::Arc::new(
                                sailry_client::conversation::details::Tracker::observe(
                                    client.clone(),
                                    binding.runtime.handle().clone(),
                                ),
                            )
                        });
                    values.insert(
                        id.clone(),
                        Observation {
                            history: history.clone(),
                            client,
                            prefix: prefix.clone(),
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
            let mut receiver = receiver.clone();
            let stop = stop.clone();
            Ok(async move {
                loop {
                    if stop.is_cancelled() {
                        return Err(HostError::new("plugin view is closed"));
                    }
                    let observed = receiver.borrow_and_update().clone();
                    let mut value = BTreeMap::new();
                    for (id, observed) in observed {
                        match read(observed, stop.clone()).await {
                            Ok(snapshot) => {
                                value.insert(id, snapshot);
                            }
                            Err(error) => {
                                if stop.is_cancelled() {
                                    return Err(error);
                                }
                                eprintln!("assistant observation failed: {error}");
                            }
                        }
                    }
                    let bytes = serde_json::to_vec(&value)
                        .map_err(|error| HostError::new(error.to_string()))?;
                    let cursor = blake3::hash(&bytes).to_hex().to_string();
                    if cursor != seen {
                        return encode(json!({"cursor":cursor,"conversations":value}));
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        changed = receiver.changed() => if changed.is_err() {
                            return Err(HostError::new("assistant subscription ended"));
                        },
                    }
                }
            })
        })
    }
}

async fn read(observation: Observation, stop: CancellationToken) -> Result<Value, HostError> {
    tokio::select! {
        _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
        value = snapshot(&observation) => value,
    }
}

async fn snapshot(observation: &Observation) -> Result<Value, HostError> {
    use sailry_client::conversation::details::State;
    let history = &observation.history;
    let Some(snapshot) = &history.snapshot else {
        observation.details.set([]);
        return Ok(json!({"connected":history.connected,"calls":[]}));
    };
    let page = &snapshot.page;
    let selected = history
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
        .collect::<Vec<_>>();
    observation.details.set(
        selected
            .iter()
            .map(|(_, _, reference, _, _)| reference.clone()),
    );
    let mut calls = Vec::new();
    let mut retained = 0;
    // Prefer recent calls when the returned snapshot itself reaches the display budget.
    for (call, name, reference, timestamp_ms, sequence) in selected.into_iter().rev() {
        let State::Ready(detail) = observation.details.wait(&reference).await else {
            continue;
        };
        let bytes = sailry_client::conversation::details::retained_bytes(&detail.arguments)
            + detail
                .result
                .as_ref()
                .map_or(0, sailry_client::conversation::details::retained_bytes);
        if retained + bytes > sailry_client::conversation::details::MAX_BYTES {
            continue;
        }
        retained += bytes;
        let result = detail.result.as_ref().map(|value| {
            serde_json::from_value(value.clone())
                .ok()
                .and_then(|output| public_output(output).ok())
                .unwrap_or_else(|| value.clone())
        });
        calls.push(json!({"key":format!("{}:{}",call.turn,call.source.key()),"name":name,"arguments":detail.arguments,
            "state":call.state,"timestamp_ms":timestamp_ms,"result":result,"sequence":sequence.map(|sequence|sequence.to_string())}));
    }
    calls.reverse();
    Ok(
        json!({"connected":history.connected,"session":page.session,"revision":page.revision.to_string(),
        "latest":page.entries.iter().map(|entry|entry.sequence).max().map(|value|value.to_string()),"calls":calls}),
    )
}

#[cfg(test)]
mod tests;
