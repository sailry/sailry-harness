use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use sailry_protocol::{Fault, NodeId};
use tokio::sync::watch;

pub(super) struct Registry {
    state: Mutex<State>,
    updates: watch::Sender<BTreeSet<NodeId>>,
}

#[derive(Default)]
struct State {
    counts: BTreeMap<NodeId, usize>,
    closed: bool,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            updates: watch::channel(BTreeSet::new()).0,
        }
    }
}

impl Registry {
    pub(super) fn subscribe(&self) -> watch::Receiver<BTreeSet<NodeId>> {
        self.updates.subscribe()
    }

    pub(super) fn enter(self: &Arc<Self>, peer: NodeId) -> Result<Guard, Fault> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(super::unavailable());
        }
        let count = state.counts.entry(peer).or_default();
        *count += 1;
        if *count == 1 {
            self.updates.send_modify(|peers| {
                peers.insert(peer);
            });
        }
        Ok(Guard {
            registry: self.clone(),
            peer,
        })
    }

    pub(super) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        state.counts.clear();
        self.updates.send_replace(BTreeSet::new());
    }
}

pub(super) struct Guard {
    registry: Arc<Registry>,
    peer: NodeId,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let mut state = self
            .registry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(count) = state.counts.get_mut(&self.peer) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            state.counts.remove(&self.peer);
            self.registry.updates.send_modify(|peers| {
                peers.remove(&self.peer);
            });
        }
    }
}
