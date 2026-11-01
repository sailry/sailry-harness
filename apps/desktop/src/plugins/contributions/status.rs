//! Node state and controller-local Status presentation share one scoped feed.
use super::*;
use sailry_client::status::{self as feed, Scope as FeedScope};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Output, Request, plugin::ui::Kind};
use serde_json::Value;
use std::{cmp::Ordering, collections::BTreeSet};
use tokio::sync::watch;

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        (
            self.node,
            self.worktree,
            self.session,
            &self.package.name,
            &self.package.digest,
            self.package.settings_revision,
            &self.id,
        )
            .cmp(&(
                other.node,
                other.worktree,
                other.session,
                &other.package.name,
                &other.package.digest,
                other.package.settings_revision,
                &other.id,
            ))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) struct Feed {
    node: NodeId,
    scope: FeedScope,
    client: Arc<sailry_client::Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    view: watch::Receiver<feed::View>,
    open: BTreeMap<Key, bool>,
    pending: BTreeMap<Key, Pending>,
    stop: CancellationToken,
    _watcher: tokio::task::JoinHandle<()>,
    _updates: Task<()>,
}

struct Pending {
    request: Request,
    active: bool,
    task: Option<Task<()>>,
}

impl Pending {
    fn retry(&self) -> Result<Request, &'static str> {
        if self.active {
            return Err("status action is busy");
        }
        Ok(self.request.clone())
    }

    fn complete(&mut self, result: &Result<Output, Fault>) -> bool {
        self.active = false;
        self.task = None;
        result
            .as_ref()
            .is_err_and(|fault| fault.code == ErrorCode::OutcomeUnknown)
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Feed {
    fn new(binding: &Binding, scope: FeedScope, cx: &mut Context<Registry>) -> Self {
        let (updates, view) = watch::channel(feed::View::default());
        let mut receiver = view.clone();
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let client = binding.client.clone();
        let watcher = binding.runtime.spawn(async move {
            client.watch_status(scope, updates, cancellation).await;
        });
        let node = binding.client.target();
        let updates = cx.spawn(async move |registry, cx| {
            while receiver.changed().await.is_ok() {
                let _ = receiver.borrow_and_update();
                if registry
                    .update(cx, |registry, cx| {
                        if let Some(feed) = &mut registry.status
                            && feed.node == node
                            && feed.scope == scope
                        {
                            feed.retain();
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            node,
            scope,
            client: binding.client.clone(),
            runtime: binding.runtime.clone(),
            view,
            open: BTreeMap::new(),
            pending: BTreeMap::new(),
            stop,
            _watcher: watcher,
            _updates: updates,
        }
    }

    fn key(&self, entry: &feed::Entry) -> Key {
        Key {
            node: self.node,
            worktree: self.scope.worktree,
            session: self.scope.session,
            package: entry.package.clone(),
            id: entry.declaration.id.clone(),
        }
    }

    fn source(&self, key: &Key) -> Option<feed::Entry> {
        if key.node != self.node
            || key.worktree != self.scope.worktree
            || key.session != self.scope.session
        {
            return None;
        }
        self.view
            .borrow()
            .entries
            .iter()
            .find(|entry| entry.package == key.package && entry.declaration.id == key.id)
            .cloned()
    }

    pub(super) fn contains(&self, key: &Key) -> bool {
        self.source(key).is_some()
    }

    fn entry(&self, source: &feed::Entry) -> Entry {
        let key = self.key(source);
        present(
            source,
            key.clone(),
            self.view.borrow().connected,
            self.open.get(&key) == Some(&true),
            self.pending.get(&key).is_some_and(|pending| pending.active),
        )
    }

    fn retain(&mut self) {
        let keys: BTreeSet<_> = self
            .view
            .borrow()
            .entries
            .iter()
            .map(|entry| self.key(entry))
            .collect();
        self.open.retain(|key, _| keys.contains(key));
        self.pending.retain(|key, _| keys.contains(key));
    }
}

fn present(source: &feed::Entry, key: Key, connected: bool, open: bool, active: bool) -> Entry {
    let mut state = source.state.clone();
    state.visible |= open;
    state.enabled &= connected && !active;
    if source.declaration.kind == Kind::Popover {
        state.value = open.into();
    }
    Entry {
        key,
        declaration: source.declaration.clone(),
        state,
        retry: false,
    }
}

pub(super) fn tooltip(details: &[ui::Detail], locale: &str) -> Option<String> {
    (!details.is_empty()).then(|| {
        details
            .iter()
            .map(|detail| {
                let label = detail.label.label(locale);
                if detail.value.is_empty() {
                    label.to_owned()
                } else {
                    format!("{label}: {}", detail.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

impl Registry {
    pub(super) fn sync_status(&mut self, binding: &Binding, cx: &mut Context<Self>) {
        let scope = FeedScope {
            worktree: self.worktree,
            session: self.session,
        };
        if self.surface != Surface::Composer || !scope.valid() {
            self.status = None;
        } else if self.status.is_none() {
            self.status = Some(Feed::new(binding, scope, cx));
        }
    }

    fn local_status(&self, entry: &feed::Entry, cx: &App) -> bool {
        entry.declaration.kind == Kind::Button && entry.invoke.is_some()
            || self
                .status
                .as_ref()
                .and_then(|feed| self.current(&feed.key(entry)))
                .is_some_and(|mounted| mounted.active(cx))
    }

    pub(super) fn status_entries(&self, cx: &App) -> Vec<Entry> {
        let Some(feed) = &self.status else {
            return vec![];
        };
        feed.view
            .borrow()
            .entries
            .iter()
            .map(|source| {
                let mut entry = feed.entry(source);
                entry.state.enabled &= self.local_status(source, cx);
                entry
            })
            .filter(|entry| entry.state.visible)
            .collect()
    }

    pub(super) fn status_intent_ready(&self, intent: &str, cx: &App) -> Option<bool> {
        let feed = self.status.as_ref()?;
        let view = feed.view.borrow();
        if let Some(entry) = view
            .entries
            .iter()
            .find(|entry| entry.declaration.intent.as_deref() == Some(intent))
        {
            let failed = self
                .current(&feed.key(entry))
                .is_some_and(|mounted| mounted.panel.read(cx).initialization.failed());
            return Some(view.connected && (self.local_status(entry, cx) || failed));
        }
        self.metadata
            .read(cx)
            .entries
            .values()
            .any(|info| {
                info.summary.available()
                    && info.extension.as_ref().is_some_and(|extension| {
                        extension.ui.iter().any(|entry| {
                            entry.slot == Slot::Status && entry.intent.as_deref() == Some(intent)
                        })
                    })
            })
            .then_some(view.connected && self.settled(cx))
    }

    pub(super) fn status_intent(&self, intent: &str, cx: &App) -> Option<Entry> {
        let feed = self.status.as_ref()?;
        feed.view.borrow().entries.iter().find_map(|source| {
            if source.declaration.intent.as_deref() != Some(intent) {
                return None;
            }
            let mut entry = feed.entry(source);
            entry.state.enabled &= self.local_status(source, cx);
            entry.state.enabled.then_some(entry)
        })
    }

    pub(super) fn close_status(&mut self, package: &Reference, ids: BTreeSet<String>) {
        if let Some(feed) = &mut self.status {
            feed.open
                .retain(|key, _| key.package != *package || !ids.contains(&key.id));
        }
    }

    pub(super) fn invoke_status(
        &mut self,
        key: &Key,
        kind: ui::EventKind,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let source = self
            .status
            .as_ref()
            .and_then(|feed| feed.source(key))
            .ok_or("status context changed")?;
        let state = self.status.as_ref().unwrap().entry(&source).state;
        let close = source.declaration.kind == Kind::Popover
            && kind == ui::EventKind::Change
            && value == Value::Bool(false);
        if close {
            self.status.as_mut().unwrap().open.remove(key);
            cx.notify();
        } else if !state.enabled {
            return Err("status contribution is unavailable".into());
        }
        match (source.declaration.kind, kind) {
            (Kind::Button, ui::EventKind::Invoke) if value.is_null() && state.visible => {
                if source.invoke.is_some() {
                    return self.send_status(key, &source, window, cx);
                }
            }
            (Kind::Popover, ui::EventKind::Invoke) if value.is_null() => {}
            (Kind::Popover, ui::EventKind::Change) if value.is_boolean() => {}
            _ => return Err("invalid status interaction".into()),
        }
        self.current(key)
            .filter(|mounted| mounted.active(cx))
            .ok_or("status panel is unavailable")?
            .bridge
            .forward(&key.id, kind, value.clone())
            .map_err(str::to_owned)?;
        if source.declaration.kind == Kind::Popover && !close {
            let open = kind == ui::EventKind::Invoke || value.as_bool() == Some(true);
            if open {
                self.status.as_mut().unwrap().open.insert(key.clone(), true);
            }
        }
        cx.notify();
        Ok(())
    }

    fn send_status(
        &mut self,
        key: &Key,
        source: &feed::Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let feed = self.status.as_mut().ok_or("status context changed")?;
        let request = match feed.pending.get(key) {
            Some(pending) => pending.retry().map_err(str::to_owned)?,
            None => feed
                .client
                .prepare_status(&feed.scope, source)
                .map_err(|fault| fault.to_string())?,
        };
        let id = request.id;
        let client = feed.client.clone();
        let sent = request.clone();
        let job = feed
            .runtime
            .spawn(async move { client.execute(sent).await });
        let expected = key.clone();
        let task = cx.spawn_in(window, async move |registry, cx| {
            let result = job.await.unwrap_or_else(|_| {
                Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "status action completion is unknown",
                ))
            });
            let _ = registry.update_in(cx, |registry, window, cx| {
                let Some(feed) = &mut registry.status else {
                    return;
                };
                let Some(pending) = feed.pending.get_mut(&expected) else {
                    return;
                };
                if pending.request.id != id {
                    return;
                }
                if !pending.complete(&result) {
                    feed.pending.remove(&expected);
                }
                if let Err(error) = result {
                    crate::feedback::error(
                        "plugins_failed",
                        &crate::tr("plugins_failed"),
                        &error.to_string(),
                        window,
                        cx,
                    );
                }
                cx.notify();
            });
        });
        feed.pending.insert(
            key.clone(),
            Pending {
                request,
                active: true,
                task: Some(task),
            },
        );
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
