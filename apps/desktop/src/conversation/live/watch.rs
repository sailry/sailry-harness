use super::*;
mod snapshot;

pub(super) fn python(
    binding: &Binding,
    stop: &CancellationToken,
    cx: &mut Context<View>,
) -> Task<()> {
    let client = binding.client.clone();
    let node = client.target();
    let stop = stop.clone();
    let (updates, mut receiver) =
        tokio::sync::watch::channel(sailry_client::python::View::default());
    binding.runtime.spawn(async move {
        let _ = client.watch_python(updates, stop).await;
    });
    cx.spawn(async move |view, cx| {
        loop {
            let next = receiver.borrow_and_update().clone();
            if view
                .update(cx, |view, cx| {
                    if view.binding.client.target() == node {
                        view.python = next;
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
            if receiver.changed().await.is_err() {
                break;
            }
        }
    })
}

impl View {
    pub(super) fn refresh_clock(&mut self, cx: &mut Context<Self>) {
        let active = self.history.connected
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .page
                    .runs
                    .iter()
                    .chain(snapshot.page.children.iter().map(|child| &child.run))
                    .any(|run| {
                        run.started_ms.is_some()
                            && matches!(run.status, Status::Running | Status::Stopping)
                    })
            });
        if !active {
            self.clock = None;
        } else if self.clock.is_none() {
            self.clock = Some(cx.spawn(async move |view, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    if view.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
    }
}

pub(super) fn node(
    binding: &Binding,
    defaults: bool,
    stop: &CancellationToken,
    cx: &mut Context<View>,
) -> Task<()> {
    let (sender, mut receiver) = tokio::sync::watch::channel(NodeView::default());
    let client = if defaults {
        binding.defaults.clone()
    } else {
        binding.client.clone()
    };
    let stop = stop.clone();
    let observation = stop.clone();
    let watcher = client.clone();
    let runtime = binding.runtime.clone();
    let node = client.target();
    binding.runtime.spawn(async move {
        let _ = watcher.watch(sender, observation).await;
    });
    cx.spawn(async move |view, cx| {
        loop {
            let mut next = receiver.borrow_and_update().clone();
            let candidate = view
                .update(cx, |view, _| {
                    if defaults || view.binding.client.target() != node {
                        return None;
                    }
                    view.session()
                        .filter(|id| snapshot::missing(&next, *id))
                        .map(|session| (session, view.binding.project, view.binding.worktree))
                })
                .ok()
                .flatten();
            let confirmed = if let Some((session, project, worktree)) = candidate {
                let client = client.clone();
                let result = runtime
                    .spawn(async move { snapshot::read(&client).await })
                    .await
                    .unwrap_or_else(|_| {
                        Err(sailry_protocol::Fault::new(
                            sailry_protocol::ErrorCode::Internal,
                            "session snapshot read stopped",
                        ))
                    });
                snapshot::confirm(&mut next, node, session, result)
                    .then_some((session, project, worktree))
            } else {
                None
            };
            if stop.is_cancelled() {
                break;
            }
            // Client owns ordering and recovery; a newer watch update supersedes this read.
            if candidate.is_some() && receiver.borrow().has_changed() {
                continue;
            }
            if view
                .update(cx, |view, cx| {
                    if !defaults {
                        if view.binding.client.target() != node {
                            return;
                        }
                        if let Some((session, project, worktree)) = candidate
                            && (view.session() != Some(session)
                                || view.binding.project != project
                                || view.binding.worktree != worktree)
                        {
                            return;
                        }
                    }
                    if let Some(session) = snapshot::removed(
                        &next,
                        confirmed.map(|(session, _, _)| session),
                        view.session(),
                    ) {
                        cx.emit(Event::Removed(session));
                        return;
                    }
                    if !defaults
                        && let Some(snapshot) = &next.snapshot
                        && let Some(session) = &view.session
                        && let Some(session) = snapshot
                            .sessions
                            .iter()
                            .find(|item| item.id == session.id && item.revision >= session.revision)
                    {
                        view.session = Some(session.clone());
                    }
                    if defaults {
                        view.defaults = next;
                    } else {
                        view.node = next;
                        view.sync_child_approvals(cx);
                        if view.outgoing.is_some() {
                            view.sync_rows(cx);
                        }
                    }
                    view.sync_location(cx);
                    view.refresh_config(cx);
                    crate::updater::recovery::request(cx);
                    if !defaults {
                        view.refresh_repository(repository::Refresh::Snapshot, cx);
                    }
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
            if receiver.changed().await.is_err() {
                break;
            }
        }
    })
}

pub(super) fn history(
    binding: &Binding,
    session: SessionId,
    initial: History,
    stop: &CancellationToken,
    cx: &mut Context<View>,
) -> (
    Task<()>,
    tokio::sync::mpsc::Sender<sailry_client::conversation::HistoryRequest>,
) {
    let initial = if initial.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.node != binding.client.target() || snapshot.page.session != session
    }) {
        History::default()
    } else {
        initial
    };
    let (sender, mut receiver) = tokio::sync::watch::channel(History {
        connected: false,
        error: None,
        loading_older: false,
        older_error: None,
        ..initial
    });
    let (older, requests) = tokio::sync::mpsc::channel(8);
    let client = binding.client.clone();
    let stop = stop.clone();
    binding.runtime.spawn(async move {
        let _ = client
            .watch_conversation(session, sender, stop, requests)
            .await;
    });
    let task = cx.spawn(async move |view, cx| {
        loop {
            let next = receiver.borrow_and_update().clone();
            if view
                .update(cx, |view, cx| {
                    if view.session() != Some(session) {
                        return;
                    }
                    if next.snapshot.is_some() || next.error.is_some() {
                        view.update_history(next, cx);
                        crate::updater::recovery::request(cx);
                    }
                })
                .is_err()
            {
                break;
            }
            if receiver.changed().await.is_err() {
                break;
            }
        }
    });
    (task, older)
}
