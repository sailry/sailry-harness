use super::*;

impl View {
    pub(super) fn refresh_clock(&mut self, cx: &mut Context<Self>) {
        let active = self.history.connected
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.runs.iter().any(|run| {
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
    binding.runtime.spawn(async move {
        let _ = client.watch(sender, stop).await;
    });
    cx.spawn(async move |view, cx| {
        loop {
            let next = receiver.borrow_and_update().clone();
            if view
                .update(cx, |view, cx| {
                    if !defaults
                        && next.connected
                        && let Some(snapshot) = &next.snapshot
                        && let Some(session) = &view.session
                        && !snapshot.sessions.iter().any(|item| item.id == session.id)
                    {
                        cx.emit(Event::Removed(session.id));
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
    stop: &CancellationToken,
    cx: &mut Context<View>,
) -> (
    Task<()>,
    tokio::sync::mpsc::Sender<sailry_client::conversation::HistoryRequest>,
) {
    let (sender, mut receiver) = tokio::sync::watch::channel(History::default());
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
