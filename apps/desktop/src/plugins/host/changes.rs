//! Coalesced invalidation from the panel's existing shared Client projection.
//! No second subscription, resource payload, history reducer or recovery loop.
use super::*;
use gpui_shell::HostObject;
use tokio::sync::{Semaphore, watch};

#[derive(Clone, Default, PartialEq, Eq)]
struct State {
    cursor: u64,
    connected: bool,
    entry: u64,
}

pub(super) struct Changes {
    state: watch::Sender<State>,
    pending: Arc<Semaphore>,
}

impl Default for Changes {
    fn default() -> Self {
        Self {
            state: watch::channel(State::default()).0,
            pending: Arc::new(Semaphore::new(1)),
        }
    }
}

impl Host {
    pub(in crate::plugins) fn observe(&self, cursor: u64, connected: bool) {
        self.changes.state.send_if_modified(|state| {
            if state.cursor == cursor && state.connected == connected {
                return false;
            }
            state.cursor = cursor;
            state.connected = connected;
            true
        });
    }

    pub(in crate::plugins) fn enter(&self) {
        self.changes.state.send_modify(|state| {
            state.entry = state.entry.wrapping_add(1);
        });
    }

    pub(super) fn change_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let owner = self.clone();
        module.async_function("nextChange", move |args| {
            owner.check()?;
            let seen = args
                .get(0)
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let mut updates = owner.changes.state.subscribe();
            let permit = owner
                .changes
                .pending
                .clone()
                .try_acquire_owned()
                .map_err(|_| HostError::new("a view change request is already pending"))?;
            let stop = owner.stop.clone();
            Ok(async move {
                let _permit = permit;
                loop {
                    if stop.is_cancelled() {
                        return Err(HostError::new("plugin view is closed"));
                    }
                    let state = updates.borrow_and_update().clone();
                    let cursor = format!("{}:{}:{}", state.cursor, state.connected, state.entry);
                    if cursor != seen {
                        return Ok(HostObject::new()
                            .field("cursor", cursor)
                            .field("connected", state.connected)
                            .field("entry", state.entry.to_string())
                            .into());
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        changed = updates.changed() => if changed.is_err() {
                            return Err(HostError::new("plugin view subscription ended"));
                        },
                    }
                }
            })
        })
    }
}
