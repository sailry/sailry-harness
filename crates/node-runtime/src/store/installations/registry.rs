use crate::installation::Reporter;
use sailry_link::{CancellationToken, Pending, Response, Subscription};
use sailry_protocol::{
    ErrorCode, Fault, NodeId, Request, RequestId, Update,
    installation::{Phase, Progress, Resource, State, Task},
    plugin::Context,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

pub(in crate::store) struct Registry {
    state: Mutex<Entries>,
    updates: watch::Sender<Vec<Observation>>,
}

#[derive(Default)]
struct Entries {
    records: BTreeMap<(NodeId, RequestId), Record>,
    position: u64,
}

struct Record {
    task: Task,
    position: u64,
    // Preserve captured authority for future scoped SDK observation, never publish it.
    plugin: Option<Context>,
}

#[derive(Clone)]
struct Observation {
    caller: NodeId,
    task: Task,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            state: Mutex::default(),
            updates: watch::channel(Vec::new()).0,
        }
    }
}

impl Registry {
    /// Called only after the Store's ordinary durable admission.
    pub(in crate::store) fn begin(
        self: &Arc<Self>,
        caller: NodeId,
        request: &Request,
        resource: Resource,
    ) -> Reporter {
        let key = (caller, request.id);
        let mut entries = self.state.lock().expect("installation state lock is valid");
        entries.position += 1;
        let position = entries.position;
        entries.records.insert(
            key,
            Record {
                task: Task {
                    request: request.id,
                    revision: 1,
                    resource,
                    state: State::Queued,
                },
                position,
                plugin: request.plugin.clone(),
            },
        );
        self.publish(&entries);
        drop(entries);
        let registry = self.clone();
        Arc::new(move |phase, progress| registry.report(key, phase, progress))
    }

    fn report(&self, key: (NodeId, RequestId), phase: Phase, progress: Option<Progress>) {
        if progress.is_some_and(|progress| progress.validate().is_err()) {
            return;
        }
        let mut entries = self.state.lock().expect("installation state lock is valid");
        let Some(record) = entries.records.get_mut(&key) else {
            return;
        };
        let state = State::Running { phase, progress };
        if record.task.state.terminal() || record.task.state == state {
            return;
        }
        record.task.revision += 1;
        record.task.state = state;
        self.publish(&entries);
    }

    /// Only the Store calls this, after recording the authoritative business result.
    pub(in crate::store) fn finish(&self, caller: NodeId, request: RequestId, result: &Response) {
        let mut entries = self.state.lock().expect("installation state lock is valid");
        let key = (caller, request);
        let Some(record) = entries.records.get_mut(&key) else {
            return;
        };
        match result {
            Ok(_) => {
                entries.records.remove(&key);
            }
            Err(error) => {
                record.task.revision += 1;
                record.task.state = if error.code == ErrorCode::OutcomeUnknown {
                    State::Uncertain {
                        error: error.clone(),
                    }
                } else {
                    State::Failed {
                        error: error.clone(),
                    }
                };
            }
        }
        self.publish(&entries);
    }

    pub(in crate::store) fn dismiss(
        &self,
        caller: NodeId,
        request: RequestId,
        plugin: Option<&Context>,
    ) -> Result<(), Fault> {
        let mut entries = self.state.lock().expect("installation state lock is valid");
        let key = (caller, request);
        let Some(record) = entries.records.get(&key) else {
            return Ok(());
        };
        if plugin.is_some() && record.plugin.as_ref() != plugin {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "installation belongs to another plugin scope",
            ));
        }
        if !record.task.state.terminal() {
            return Err(Fault::new(ErrorCode::Busy, "installation is still active"));
        }
        entries.records.remove(&key);
        self.publish(&entries);
        Ok(())
    }

    fn publish(&self, entries: &Entries) {
        let mut records = entries.records.iter().collect::<Vec<_>>();
        records.sort_by_key(|(_, record)| record.position);
        self.updates.send_replace(
            records
                .into_iter()
                .map(|((caller, _), record)| Observation {
                    caller: *caller,
                    task: record.task.clone(),
                })
                .collect(),
        );
    }

    pub(in crate::store) fn subscribe(
        &self,
        node: NodeId,
        caller: NodeId,
        closed: CancellationToken,
    ) -> Box<dyn Subscription> {
        Box::new(Updates {
            node,
            caller,
            updates: self.updates.subscribe(),
            closed,
            initial: true,
        })
    }
}

struct Updates {
    node: NodeId,
    caller: NodeId,
    updates: watch::Receiver<Vec<Observation>>,
    closed: CancellationToken,
    initial: bool,
}

impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(super::super::unavailable());
            }
            if !self.initial {
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(super::super::unavailable()),
                    changed = self.updates.changed() => changed.map_err(|_| super::super::unavailable())?,
                }
            }
            self.initial = false;
            Ok(Update::Installations {
                node: self.node,
                tasks: self
                    .updates
                    .borrow_and_update()
                    .iter()
                    .filter(|record| record.caller == self.caller)
                    .map(|record| record.task.clone())
                    .collect(),
            })
        })
    }
}

#[cfg(test)]
mod tests;
