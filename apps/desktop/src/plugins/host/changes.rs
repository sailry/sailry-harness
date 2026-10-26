//! Coalesced invalidation from the panel's existing shared Client projection.
//! Repository invalidation reuses Client's scoped file watcher and recovery.
use super::*;
use gpui_shell::HostObject;
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

const DEBOUNCE: Duration = Duration::from_millis(750);
const MAX_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
enum Scope {
    Node = 0,
    Package = 1,
    Settled = 2,
    Repository = 3,
}

impl Scope {
    fn read(value: Option<&HostValue>) -> Result<Self, HostError> {
        match value {
            None => Ok(Self::Node),
            Some(value) if value.as_str() == Some("package") => Ok(Self::Package),
            Some(value) if value.as_str() == Some("settled") => Ok(Self::Settled),
            Some(value) if value.as_str() == Some("repository") => Ok(Self::Repository),
            _ => Err(HostError::new("invalid change scope")),
        }
    }
}

#[derive(Clone)]
struct State {
    cursors: [u64; 4],
    updated: [Option<Instant>; 4],
    connected: bool,
    entry: u64,
    visible: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cursors: [0; 4],
            updated: [None; 4],
            connected: false,
            entry: 0,
            visible: true,
        }
    }
}

impl State {
    fn suffix(&self) -> String {
        format!("{}:{}:{}", self.connected, self.entry, self.visible)
    }

    fn token(&self, scope: Scope, files: u64) -> String {
        format!("{}-{files}:{}", self.cursors[scope as usize], self.suffix())
    }

    fn deadline(&self, scope: Scope, first: Instant) -> Instant {
        (self.updated[scope as usize].unwrap_or(first) + DEBOUNCE).min(first + MAX_WAIT)
    }

    fn value(&self, cursor: String) -> HostValue {
        HostObject::new()
            .field("cursor", cursor)
            .field("connected", self.connected)
            .field("entry", self.entry.to_string())
            .field("visible", self.visible)
            .into()
    }
}

pub(super) struct Changes {
    state: watch::Sender<State>,
    files: std::sync::OnceLock<watch::Receiver<sailry_client::FileView>>,
}

impl Default for Changes {
    fn default() -> Self {
        Self {
            state: watch::channel(State::default()).0,
            files: Default::default(),
        }
    }
}

#[cfg(test)]
impl Changes {
    pub(super) fn waiting(&self) -> usize {
        self.state.receiver_count()
    }
}

impl Host {
    pub(in crate::plugins) fn observe(
        &self,
        cursor: u64,
        package: u64,
        settled: u64,
        connected: bool,
    ) {
        self.changes.state.send_if_modified(|state| {
            let cursors = [cursor, package, settled, settled];
            let mut changed = state.connected != connected;
            for (index, cursor) in cursors.into_iter().enumerate() {
                if state.cursors[index] != cursor {
                    state.cursors[index] = cursor;
                    state.updated[index] = Some(Instant::now());
                    changed = true;
                }
            }
            state.connected = connected;
            changed
        });
    }

    pub(in crate::plugins) fn set_visible(&self, visible: bool) {
        self.changes.state.send_if_modified(|state| {
            if state.visible == visible {
                return false;
            }
            state.visible = visible;
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
            let scope = Scope::read(args.get(1))?;
            if matches!(scope, Scope::Settled | Scope::Repository)
                && owner.context.worktree.is_none()
            {
                return Err(HostError::new("change scope requires a worktree"));
            }
            let mut files = if matches!(scope, Scope::Repository) {
                Some(
                    owner
                        .changes
                        .files
                        .get_or_init(|| {
                            let (sender, receiver) =
                                watch::channel(sailry_client::FileView::default());
                            let client = owner.client.clone();
                            let stop = owner.stop.clone();
                            let worktree =
                                owner.context.worktree.expect("validated worktree scope");
                            owner.runtime.spawn(async move {
                                let _ = client.watch_repository(worktree, sender, stop).await;
                            });
                            receiver
                        })
                        .clone(),
                )
            } else {
                None
            };
            let mut updates = owner.changes.state.subscribe();
            let stop = owner.stop.clone();
            let runtime = owner.runtime.clone();
            Ok(async move {
                let mut first = None;
                let mut file_revision = None;
                let mut file_updated = None;
                loop {
                    if stop.is_cancelled() {
                        return Err(HostError::new("plugin view is closed"));
                    }
                    let mut state = updates.borrow_and_update().clone();
                    let revision = files
                        .as_mut()
                        .map(|files| {
                            let value = files.borrow_and_update().clone();
                            state.connected &= value.connected;
                            value.revision
                        })
                        .unwrap_or(0);
                    if file_revision != Some(revision) {
                        file_revision = Some(revision);
                        file_updated = Some(Instant::now());
                    }
                    if matches!(scope, Scope::Repository) {
                        state.updated[scope as usize] =
                            state.updated[scope as usize].max(file_updated);
                    }
                    let cursor = state.token(scope, revision);
                    let urgent = seen.is_empty()
                        || seen.split_once(':').map(|(_, suffix)| suffix)
                            != Some(state.suffix().as_str());
                    let deadline = if cursor != seen && (state.visible || urgent) {
                        if urgent {
                            return Ok(state.value(cursor));
                        }
                        let deadline =
                            state.deadline(scope, *first.get_or_insert_with(Instant::now));
                        if Instant::now() >= deadline {
                            return Ok(state.value(cursor));
                        }
                        Some(deadline)
                    } else {
                        first = None;
                        None
                    };
                    let timer = async {
                        if let Some(deadline) = deadline {
                            let sleep = {
                                let _runtime = runtime.enter();
                                tokio::time::sleep_until(deadline)
                            };
                            sleep.await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    };
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        _ = timer => {},
                        changed = updates.changed() => if changed.is_err() {
                            return Err(HostError::new("plugin view subscription ended"));
                        },
                        changed = async {
                            if let Some(files) = &mut files { files.changed().await }
                            else { std::future::pending().await }
                        } => if changed.is_err() {
                            return Err(HostError::new("plugin file subscription ended"));
                        },
                    }
                }
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_trailing_debounce() {
        let first = Instant::now();
        let mut state = State::default();
        state.updated[0] = Some(first);
        assert_eq!(state.deadline(Scope::Node, first), first + DEBOUNCE);
        state.updated[0] = Some(first + Duration::from_millis(600));
        assert_eq!(state.deadline(Scope::Node, first), first + MAX_WAIT);
        state.updated[1] = Some(first);
        assert_eq!(state.deadline(Scope::Package, first), first + DEBOUNCE);
    }
}
