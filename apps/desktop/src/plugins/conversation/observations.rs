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
    runtime: tokio::runtime::Handle,
    prefix: String,
    cache: std::sync::Arc<tokio::sync::Mutex<Cache>>,
}

struct Captured {
    stamp: Stamp,
    value: Option<Box<str>>,
    bytes: usize,
    required: usize,
}

const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Cache {
    values: BTreeMap<String, Captured>,
    bytes: usize,
}

impl Cache {
    fn clear(&mut self) {
        self.values.clear();
        self.bytes = 0;
    }

    fn retain(&mut self, retained: &BTreeMap<String, Stamp>) {
        self.values.retain(|key, entry| {
            retained
                .get(key)
                .is_some_and(|stamp| stamp.tool == entry.stamp.tool)
        });
        self.bytes = self.values.values().map(|entry| entry.bytes).sum();
    }

    fn check(&self) -> Result<(), HostError> {
        if self
            .values
            .values()
            .map(|entry| entry.required)
            .sum::<usize>()
            > MAX_CAPTURE_BYTES
        {
            Err(HostError::new(
                "assistant tool snapshot exceeds cache capacity",
            ))
        } else {
            Ok(())
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(captured) = self.values.remove(key) {
            self.bytes -= captured.bytes;
        }
    }

    fn insert(&mut self, key: String, stamp: Stamp, value: &Value) -> Result<(), HostError> {
        self.remove(&key);
        let encoded = serde_json::to_string(value)
            .map_err(|error| HostError::new(error.to_string()))?
            .into_boxed_str();
        let metadata =
            stamp.tool.retained_bytes() + key.capacity() + std::mem::size_of::<Captured>();
        let required = metadata + encoded.len();
        let fits = self
            .values
            .values()
            .map(|entry| entry.required)
            .sum::<usize>()
            + required
            <= MAX_CAPTURE_BYTES;
        if !fits {
            for captured in self.values.values_mut() {
                if let Some(value) = captured.value.take() {
                    captured.bytes -= value.len();
                    self.bytes -= value.len();
                }
            }
        }
        let value = fits.then_some(encoded);
        let bytes = metadata + value.as_ref().map_or(0, |value| value.len());
        self.bytes += bytes;
        self.values.insert(
            key,
            Captured {
                stamp,
                value,
                bytes,
                required,
            },
        );
        self.check()
    }
}

#[derive(PartialEq, Eq)]
struct Stamp {
    tool: sailry_client::conversation::tools::Stamp,
    timestamp_ms: i64,
    sequence: Option<u64>,
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
                    let cache = values
                        .get(&id)
                        .filter(|previous| {
                            previous.client.target() == client.target() && previous.prefix == prefix
                        })
                        .map(|previous| previous.cache.clone())
                        .unwrap_or_default();
                    values.insert(
                        id.clone(),
                        Observation {
                            history: history.clone(),
                            client,
                            runtime: binding.runtime.handle().clone(),
                            prefix: prefix.clone(),
                            cache,
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
                        let snapshot = read(observed, stop.clone()).await?;
                        value.insert(id, snapshot);
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
    let runtime = observation.runtime.clone();
    runtime
        .spawn(async move {
            tokio::select! {
                biased;
                _ = stop.cancelled() => Err(HostError::new("plugin view is closed")),
                value = snapshot(&observation) => value,
            }
        })
        .await
        .map_err(|_| HostError::new("assistant observation stopped"))?
}

async fn snapshot(observation: &Observation) -> Result<Value, HostError> {
    let history = &observation.history;
    let Some(snapshot) = &history.snapshot else {
        observation.cache.lock().await.clear();
        return Ok(json!({"connected":history.connected,"calls":[]}));
    };
    let page = &snapshot.page;
    let mut calls = Vec::new();
    let mut retained = history
        .calls
        .iter()
        .filter(|call| call.name.starts_with(&observation.prefix))
        .filter_map(|call| {
            let source = page
                .entries
                .iter()
                .find(|entry| entry.id == call.source.entry)?;
            let response = call
                .response
                .as_ref()
                .and_then(|response| page.entries.iter().find(|entry| entry.id == response.entry));
            Some((
                format!("{}:{}", call.turn, call.source.key()),
                Stamp {
                    tool: call.stamp(page)?,
                    timestamp_ms: source.timestamp_ms,
                    sequence: response.map(|entry| entry.sequence),
                },
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut cache = observation.cache.lock().await;
    cache.retain(&retained);
    cache.check()?;
    for call in history.calls.iter() {
        let Some(name) = call.name.strip_prefix(&observation.prefix) else {
            continue;
        };
        let key = format!("{}:{}", call.turn, call.source.key());
        let Some(stamp) = retained.remove(&key) else {
            continue;
        };
        if let Some(previous) = cache
            .values
            .get(&key)
            .filter(|previous| previous.stamp == stamp)
        {
            if let Some(value) = &previous.value {
                calls.push(
                    serde_json::from_str(value)
                        .map_err(|error| HostError::new(error.to_string()))?,
                );
                continue;
            }
        }
        let previous = cache
            .values
            .get(&key)
            .filter(|previous| previous.value.is_some());
        let changed = match observation
            .client
            .changed_tool_call(call, page, previous.map(|previous| &previous.stamp.tool))
            .await
        {
            Ok(changed) => changed,
            Err(error) => {
                eprintln!(
                    "assistant tool detail read failed for {}:{}: {}",
                    page.session,
                    call.source.key(),
                    error.message
                );
                cache.remove(&key);
                continue;
            }
        };
        let Some((hydrated, _)) = changed else {
            let mut value: Value = serde_json::from_str(
                previous
                    .expect("unchanged captured tool")
                    .value
                    .as_deref()
                    .unwrap(),
            )
            .map_err(|error| HostError::new(error.to_string()))?;
            value["timestamp_ms"] = json!(stamp.timestamp_ms);
            value["sequence"] = json!(stamp.sequence.map(|sequence| sequence.to_string()));
            calls.push(value.clone());
            cache.insert(key, stamp, &value)?;
            continue;
        };
        let detail = hydrated.detail.as_ref().expect("hydrated tool detail");
        let result = detail.result.as_ref().map(|value| {
            serde_json::from_value(value.clone())
                .ok()
                .and_then(|output| public_output(output).ok())
                .unwrap_or_else(|| value.clone())
        });
        let value = json!({"key":key,"name":name,"arguments":detail.arguments,
            "state":call.state,"timestamp_ms":stamp.timestamp_ms,"result":result,"sequence":stamp.sequence.map(|sequence|sequence.to_string())});
        calls.push(value.clone());
        cache.insert(key, stamp, &value)?;
    }
    Ok(
        json!({"connected":history.connected,"session":page.session,"revision":page.revision.to_string(),
        "latest":page.entries.iter().map(|entry|entry.sequence).max().map(|value|value.to_string()),"calls":calls}),
    )
}

#[cfg(test)]
mod tests;
