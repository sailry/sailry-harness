use super::*;
use sailry_protocol::{Event, EventEnvelope, plugin, plugin::host::Change};
use std::{future, path::PathBuf, time::Duration};
use tokio::{
    sync::broadcast,
    time::{Instant, sleep_until},
};

const DEBOUNCE: Duration = Duration::from_millis(750);
const MAX_WAIT: Duration = Duration::from_secs(1);
const FAILURE_WAIT: Duration = Duration::from_secs(30);

pub(super) async fn run(
    ingress: Ingress,
    scope: Scope,
    updates: watch::Sender<Option<Frame>>,
    mut events: broadcast::Receiver<EventEnvelope>,
    mut signals: mpsc::UnboundedReceiver<Signal>,
    permits: Arc<Semaphore>,
    stop: CancellationToken,
) {
    let (completed, mut results) = mpsc::unbounded_channel();
    let mut runner = Runner {
        ingress,
        scope,
        effective: scope,
        packages: BTreeMap::new(),
        generation: 0,
        repository: None,
        root: None,
        updates,
        permits,
        completed,
        stop,
    };
    if let Err(error) = runner.refresh(None).await {
        runner.finish(error);
        return;
    }
    loop {
        runner.launch();
        let deadline = runner.deadline();
        tokio::select! {
            biased;
            _ = runner.stop.cancelled() => return,
            event = events.recv() => {
                let result = match event {
                    Ok(event) => runner.event(event.event).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        match runner.refresh(None).await {
                            Ok(()) => {runner.mark(None, false); Ok(())},
                            Err(error) => Err(error),
                        }
                    },
                    Err(broadcast::error::RecvError::Closed) => Err(unavailable()),
                };
                if let Err(error) = result {
                    runner.finish(error);
                    return;
                }
            },
            signal = signals.recv() => match signal {
                Some(Signal::Subscribe) => runner.mark(None, true),
                Some(Signal::Invoke(name)) => runner.mark(Some(&name), true),
                None => return,
            },
            result = next_repository(&mut runner.repository) => match result {
                Ok(_) => runner.changed(Change::Repository),
                Err(error) => {runner.finish(error); return;},
            },
            result = results.recv() => {
                if let Some(result) = result {
                    runner.complete(result).await;
                }
            },
            _ = async {
                if let Some(deadline) = deadline {
                    sleep_until(deadline).await;
                } else {
                    future::pending::<()>().await;
                }
            } => {},
        }
    }
}

async fn next_repository(repository: &mut Option<Box<dyn Subscription>>) -> Result<Update, Fault> {
    match repository {
        Some(repository) => repository.next().await,
        None => future::pending().await,
    }
}

struct Runner {
    ingress: Ingress,
    scope: Scope,
    effective: Scope,
    packages: BTreeMap<String, Package>,
    generation: u64,
    repository: Option<Box<dyn Subscription>>,
    root: Option<PathBuf>,
    updates: watch::Sender<Option<Frame>>,
    permits: Arc<Semaphore>,
    completed: mpsc::UnboundedSender<Completed>,
    stop: CancellationToken,
}

struct Package {
    info: plugin::Info,
    generation: u64,
    entries: Vec<Entry>,
    failure: Option<Fault>,
    reported: bool,
    retry_after: Option<Instant>,
    dirty: Option<Dirty>,
    running: bool,
    stop: CancellationToken,
}

impl Drop for Package {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Completed {
    name: String,
    generation: u64,
    result: Result<Vec<Entry>, Fault>,
}

#[derive(Clone, Copy)]
struct Dirty {
    first: Instant,
    latest: Instant,
    force: bool,
}

impl Dirty {
    fn new(now: Instant, force: bool) -> Self {
        Self {
            first: now,
            latest: now,
            force,
        }
    }

    fn due(self, retry_after: Option<Instant>) -> Instant {
        if self.force {
            self.latest
        } else {
            (self.latest + DEBOUNCE)
                .min(self.first + MAX_WAIT)
                .max(retry_after.unwrap_or(self.first))
        }
    }
}

impl Package {
    fn changes(&self, change: Change) -> bool {
        self.info
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .and_then(|host| host.status.as_ref())
            .is_some_and(|status| status.changes.contains(&change))
    }

    fn mark(&mut self, force: bool) {
        let now = Instant::now();
        let dirty = self.dirty.get_or_insert_with(|| Dirty::new(now, force));
        dirty.latest = now;
        dirty.force |= force;
    }

    fn deadline(&self) -> Option<Instant> {
        (!self.running)
            .then(|| self.dirty.map(|dirty| dirty.due(self.retry_after)))
            .flatten()
    }

    fn complete(&mut self, result: Result<Vec<Entry>, Fault>, now: Instant) -> Option<Fault> {
        self.running = false;
        match result {
            Ok(entries) => {
                self.entries = entries;
                self.failure = None;
                self.reported = false;
                self.retry_after = None;
                None
            }
            Err(error) => {
                let retry = error.code == ErrorCode::Busy && self.failure.is_none();
                self.failure = Some(evaluation::public(error.clone()));
                self.retry_after = Some(now + FAILURE_WAIT);
                if retry {
                    self.dirty = Some(Dirty::new(now, false));
                    None
                } else if self.reported {
                    None
                } else {
                    self.reported = true;
                    Some(error)
                }
            }
        }
    }
}

impl Runner {
    async fn refresh(&mut self, changed: Option<&str>) -> Result<(), Fault> {
        let effective = resolve(&self.ingress, self.scope).await?;
        let selected = evaluation::packages(&self.ingress, effective).await?;
        if effective != self.effective {
            self.packages.clear();
            self.repository = None;
            self.root = None;
            self.effective = effective;
        }
        self.packages
            .retain(|name, _| selected.iter().any(|info| &info.summary.name == name));
        for info in selected {
            let name = info.summary.name.clone();
            if let Some(package) = self.packages.get_mut(&name)
                && package.info.summary.reference() == info.summary.reference()
            {
                package.info = info;
                if changed == Some(name.as_str()) {
                    package.mark(true);
                }
                continue;
            }
            self.generation += 1;
            self.packages.insert(
                name,
                Package {
                    entries: evaluation::entries(&info, vec![]),
                    info,
                    generation: self.generation,
                    failure: None,
                    reported: false,
                    retry_after: None,
                    dirty: Some(Dirty::new(Instant::now(), true)),
                    running: false,
                    stop: self.stop.child_token(),
                },
            );
        }
        // Revocation and reference replacement publish before starting any callback.
        self.publish(None);
        self.repository().await
    }

    async fn repository(&mut self) -> Result<(), Fault> {
        let worktree = self.effective.worktree.filter(|_| {
            self.packages
                .values()
                .any(|package| package.changes(Change::Repository))
        });
        let Some(worktree) = worktree else {
            self.repository = None;
            self.root = None;
            return Ok(());
        };
        let root = self.ingress.worktree_root(worktree).await?;
        if self.root.as_ref() == Some(&root) && self.repository.is_some() {
            return Ok(());
        }
        self.repository = None;
        let watches = self.ingress.watches.clone();
        let node = self.ingress.node;
        let stop = self.stop.clone();
        let expected = root.clone();
        let mut subscription = tokio::task::spawn_blocking(move || {
            watches.subscribe(root, node, worktree, true, stop)
        })
        .await
        .map_err(|_| unavailable())??;
        if self.ingress.worktree_root(worktree).await? != expected {
            return Err(unavailable());
        }
        // A newly attached filesystem subscription has an initial invalidation.
        subscription.next().await?;
        self.root = Some(expected);
        self.repository = Some(subscription);
        Ok(())
    }

    async fn event(&mut self, event: Event) -> Result<(), Fault> {
        match event {
            Event::WorktreeRemoved { id } if self.effective.worktree == Some(id) => Err(
                Fault::new(ErrorCode::NotFound, "status worktree was removed"),
            ),
            Event::SessionsRemoved(ids)
                if self.scope.session.is_some_and(|id| ids.contains(&id)) =>
            {
                Err(Fault::new(
                    ErrorCode::NotFound,
                    "status session was removed",
                ))
            }
            Event::PluginChanged(summary) => self.refresh(Some(&summary.name)).await,
            Event::PluginRemoved { name } => self.refresh(Some(&name)).await,
            Event::PluginValuesChanged { name } => {
                if let Some(package) = self.packages.get_mut(&name)
                    && package.changes(Change::Package)
                {
                    package.mark(false);
                }
                Ok(())
            }
            Event::SessionChanged(session) if self.scope.session == Some(session.id) => {
                self.refresh(None).await?;
                self.changed(Change::Conversation);
                Ok(())
            }
            Event::ConversationChanged { session, .. } | Event::ToolsCompleted { session }
                if self.scope.session == Some(session) =>
            {
                self.changed(Change::Conversation);
                Ok(())
            }
            Event::TurnQueued { turn, .. } if self.scope.session == Some(turn.session) => {
                self.changed(Change::Conversation);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn mark(&mut self, name: Option<&str>, force: bool) {
        for (package_name, package) in &mut self.packages {
            if name.is_none_or(|name| name == package_name) {
                package.mark(force);
            }
        }
    }

    fn changed(&mut self, change: Change) {
        for package in self.packages.values_mut() {
            if package.changes(change) {
                package.mark(false);
            }
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.packages.values().filter_map(Package::deadline).min()
    }

    fn launch(&mut self) {
        let now = Instant::now();
        for (name, package) in &mut self.packages {
            if !package.deadline().is_some_and(|deadline| deadline <= now) {
                continue;
            }
            package.running = true;
            package.dirty = None;
            let ingress = self.ingress.clone();
            let scope = self.effective;
            let info = package.info.clone();
            let permits = self.permits.clone();
            let stop = package.stop.clone();
            let completed = self.completed.clone();
            let generation = package.generation;
            let name = name.clone();
            tokio::spawn(async move {
                let result = evaluation::read(ingress, scope, info, permits, stop).await;
                let _ = completed.send(Completed {
                    name,
                    generation,
                    result,
                });
            });
        }
    }

    async fn complete(&mut self, completed: Completed) {
        let Some(package) = self.packages.get_mut(&completed.name) else {
            return;
        };
        if package.generation != completed.generation {
            return;
        }
        let report = package
            .complete(completed.result, Instant::now())
            .map(|error| (error, package.info.clone()));
        if let Some((error, info)) = report {
            self.ingress
                .script_failure(self.ingress.node, error, &info)
                .await;
        }
        self.publish(None);
    }

    fn publish(&self, terminal: Option<Fault>) {
        let frame = Frame {
            snapshot: Snapshot {
                node: self.ingress.node,
                scope: self.scope,
                entries: self
                    .packages
                    .values()
                    .flat_map(|package| package.entries.clone())
                    .collect(),
                failures: self
                    .packages
                    .values()
                    .filter_map(|package| {
                        package.failure.as_ref().map(|fault| Failure {
                            package: package.info.summary.reference(),
                            fault: fault.clone(),
                        })
                    })
                    .collect(),
            },
            terminal: terminal.map(evaluation::public),
        };
        self.updates.send_if_modified(|previous| {
            if previous.as_ref() == Some(&frame) {
                false
            } else {
                *previous = Some(frame);
                true
            }
        });
    }

    fn finish(&mut self, error: Fault) {
        if matches!(error.code, ErrorCode::NotFound | ErrorCode::WrongTarget) {
            self.packages.clear();
        }
        self.repository = None;
        self.publish(Some(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> Package {
        let info = super::super::tests::package();
        let states =
            evaluation::validate(&info, serde_json::json!([super::super::tests::state()])).unwrap();
        Package {
            entries: evaluation::entries(&info, states),
            info,
            generation: 1,
            failure: None,
            reported: false,
            retry_after: None,
            dirty: None,
            running: true,
            stop: CancellationToken::new(),
        }
    }

    #[test]
    fn retries_busy_once_without_losing_states() {
        let mut package = package();
        let expected = package.entries.clone();
        let now = Instant::now();
        let busy = Fault::new(ErrorCode::Busy, "Git inspection queue is busy");
        assert!(package.complete(Err(busy.clone()), now).is_none());
        assert_eq!(package.entries, expected);
        assert_eq!(package.failure, Some(busy.clone()));
        assert_eq!(package.deadline(), Some(now + FAILURE_WAIT));
        package.mark(false);
        assert_eq!(package.deadline(), Some(now + FAILURE_WAIT));

        package.dirty = None;
        package.running = true;
        assert_eq!(
            package.complete(Err(busy.clone()), now + FAILURE_WAIT),
            Some(busy.clone())
        );
        assert_eq!(package.entries, expected);
        assert!(package.deadline().is_none());
        assert!(package.complete(Err(busy.clone()), now).is_none());
        assert!(package.deadline().is_none());

        assert!(package.complete(Ok(expected.clone()), now).is_none());
        assert!(package.failure.is_none());
        assert!(!package.reported);
        assert!(package.retry_after.is_none());
        assert!(package.complete(Err(busy), now).is_none());
        assert_eq!(package.deadline(), Some(now + FAILURE_WAIT));
        assert_eq!(package.entries, expected);
    }

    #[test]
    fn reports_other_failures_once_per_cycle() {
        let mut package = package();
        let expected = package.entries.clone();
        let now = Instant::now();
        let failure = Fault::invalid("Status output does not match its declarations");
        assert_eq!(
            package.complete(Err(failure.clone()), now),
            Some(failure.clone())
        );
        assert_eq!(package.entries, expected);
        assert!(package.deadline().is_none());
        assert!(package.complete(Err(failure.clone()), now).is_none());
        assert!(package.complete(Ok(expected), now).is_none());
        assert_eq!(package.complete(Err(failure.clone()), now), Some(failure));
    }

    #[test]
    fn bounds_dirty_windows_and_failure_waits() {
        let now = Instant::now();
        let mut dirty = Dirty::new(now, false);
        assert_eq!(dirty.due(None), now + DEBOUNCE);
        dirty.latest = now + Duration::from_millis(600);
        assert_eq!(dirty.due(None), now + MAX_WAIT);
        assert_eq!(dirty.due(Some(now + FAILURE_WAIT)), now + FAILURE_WAIT);
        dirty.force = true;
        assert_eq!(dirty.due(Some(now + FAILURE_WAIT)), dirty.latest);
    }
}
