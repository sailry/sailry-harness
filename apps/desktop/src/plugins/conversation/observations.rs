//! Mounted assistants expose the existing Client projection, never a second history.
use super::*;
use crate::plugins::host::sdk::{public_output, values::encode};
use gpui_shell::{HostError, HostValue};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct Observations(watch::Sender<BTreeMap<String, Value>>);

impl Default for Observations {
    fn default() -> Self {
        Self(watch::channel(BTreeMap::new()).0)
    }
}

impl Observations {
    pub fn attach(&self, id: String, package: &str, chat: &Entity<Chat>, cx: &mut App) {
        let prefix = format!(
            "plugin_{}_",
            &blake3::hash(package.as_bytes()).to_hex()[..16]
        );
        let sender = self.0.clone();
        let publish = move |chat: &Entity<Chat>, cx: &App| {
            let value = snapshot(chat.read(cx).tool_history(), &prefix);
            sender.send_if_modified(|values| {
                if values.get(&id) == Some(&value) {
                    false
                } else {
                    values.insert(id.clone(), value);
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
                    let value = receiver.borrow_and_update().clone();
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

fn snapshot(history: &sailry_client::conversation::View, prefix: &str) -> Value {
    let Some(snapshot) = &history.snapshot else {
        return json!({"connected":history.connected,"calls":[]});
    };
    let page = &snapshot.page;
    let calls = history.calls.iter().filter_map(|call| {
        let name = call.name.strip_prefix(prefix)?;
        let arguments = call.arguments(page)?;
        let source = page.entries.iter().find(|entry|entry.id == call.source.entry)?;
        let response = call.response.as_ref().and_then(|response|page.entries.iter().find(|entry|entry.id == response.entry));
        let result = call.result(page).map(|value| {
            serde_json::from_value(value.clone()).ok().and_then(|output|public_output(output).ok()).unwrap_or_else(||value.clone())
        });
        Some(json!({"key":format!("{}:{}",call.turn,call.source.key()),"name":name,"arguments":arguments,
            "state":call.state,"timestamp_ms":source.timestamp_ms,"result":result,"sequence":response.map(|entry|entry.sequence.to_string())}))
    }).collect::<Vec<_>>();
    json!({"connected":history.connected,"session":page.session,"revision":page.revision.to_string(),
        "latest":page.entries.iter().map(|entry|entry.sequence).max().map(|value|value.to_string()),"calls":calls})
}
