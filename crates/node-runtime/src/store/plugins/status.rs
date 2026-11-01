//! Status is a transient projection shared by controllers observing one scope.
use crate::store::{Ingress, commands, unavailable};
use sailry_link::{CancellationToken, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, Update, plugin::ui::status::*};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{Semaphore, mpsc, watch};

mod evaluation;
mod runner;
#[cfg(test)]
mod tests;

pub(crate) struct Service {
    watchers: Mutex<BTreeMap<Scope, Weak<Watcher>>>,
    permits: Arc<Semaphore>,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            watchers: Mutex::default(),
            permits: Arc::new(Semaphore::new(4)),
        }
    }
}

struct Watcher {
    updates: watch::Sender<Option<Frame>>,
    signals: mpsc::UnboundedSender<Signal>,
    stop: CancellationToken,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Frame {
    snapshot: Snapshot,
    terminal: Option<Fault>,
}

enum Signal {
    Subscribe,
    Invoke(String),
}

impl Service {
    pub(in crate::store) async fn subscribe(
        &self,
        ingress: Ingress,
        scope: Scope,
    ) -> Result<Box<dyn Subscription>, Fault> {
        // Capture commits before resolving the scope or selecting package code.
        let events = ingress.events.subscribe();
        resolve(&ingress, scope).await?;
        let watcher = {
            let mut watchers = self.watchers.lock().unwrap();
            watchers.retain(|_, watcher| watcher.strong_count() > 0);
            if let Some(watcher) = watchers.get(&scope).and_then(Weak::upgrade) {
                let _ = watcher.signals.send(Signal::Subscribe);
                watcher
            } else {
                let (updates, _) = watch::channel(None);
                let (signals, receiver) = mpsc::unbounded_channel();
                let stop = ingress.closed.child_token();
                let watcher = Arc::new(Watcher {
                    updates: updates.clone(),
                    signals,
                    stop: stop.clone(),
                });
                watchers.insert(scope, Arc::downgrade(&watcher));
                tokio::spawn(runner::run(
                    ingress.clone(),
                    scope,
                    updates,
                    events,
                    receiver,
                    self.permits.clone(),
                    stop,
                ));
                watcher
            }
        };
        Ok(Box::new(Feed {
            updates: watcher.updates.subscribe(),
            _watcher: watcher,
            initial: true,
            terminal: None,
            closed: ingress.closed,
        }))
    }

    pub(in crate::store) fn invalidate(&self, name: &str) {
        let mut watchers = self.watchers.lock().unwrap();
        watchers.retain(|_, watcher| {
            if let Some(watcher) = watcher.upgrade() {
                let _ = watcher.signals.send(Signal::Invoke(name.into()));
                true
            } else {
                false
            }
        });
    }
}

async fn resolve(ingress: &Ingress, scope: Scope) -> Result<Scope, Fault> {
    if !scope.valid() {
        return Err(Fault::invalid("status requires a worktree or session"));
    }
    let mut effective = scope;
    if let Some(id) = scope.session {
        let session = ingress
            .storage
            .get()
            .ok_or_else(unavailable)?
            .read(move |connection| {
                Box::pin(async move { commands::session(connection, id).await })
            })
            .await?;
        if scope
            .worktree
            .is_some_and(|worktree| worktree != session.worktree)
        {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "status session belongs to another worktree",
            ));
        }
        effective.worktree = Some(session.worktree);
    }
    if let Some(worktree) = effective.worktree {
        ingress.worktree_root(worktree).await?;
    }
    Ok(effective)
}

struct Feed {
    _watcher: Arc<Watcher>,
    updates: watch::Receiver<Option<Frame>>,
    initial: bool,
    terminal: Option<Fault>,
    closed: CancellationToken,
}

impl Subscription for Feed {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if let Some(error) = &self.terminal {
                return Err(error.clone());
            }
            loop {
                if self.initial {
                    let current = self.updates.borrow_and_update().clone();
                    if let Some(frame) = current {
                        self.initial = false;
                        self.terminal = frame.terminal;
                        return Ok(Update::Status(frame.snapshot));
                    }
                }
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(unavailable()),
                    result = self.updates.changed() => result.map_err(|_| unavailable())?,
                }
                self.initial = true;
            }
        })
    }
}
