use super::*;
use crate::dictation::models::Manager;
use sailry_protocol::{ModelId, models::Kind};

pub(super) struct Source {
    manager: Arc<Manager>,
    runtime: Arc<tokio::runtime::Runtime>,
    _watch: Task<()>,
}

pub(crate) fn controller(
    manager: Arc<Manager>,
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut App,
) {
    registry(cx).update(cx, |queue, cx| {
        queue.bind_controller(manager, runtime, cx);
        queue.sync_controller(cx);
        queue.open(window, cx);
    });
}

pub(crate) fn external(
    manager: Arc<Manager>,
    runtime: Arc<tokio::runtime::Runtime>,
    id: Option<ModelId>,
    request: RequestId,
    window: &mut Window,
    cx: &mut App,
) -> tokio::sync::oneshot::Sender<Result<(), Fault>> {
    let queue = registry(cx);
    let key = (Location::Controller, request);
    queue.update(cx, |queue, cx| {
        queue.bind_controller(manager.clone(), runtime.clone(), cx);
        let order = queue
            .entries
            .get(&key)
            .map(|entry| entry.order)
            .unwrap_or_else(|| {
                queue.next_order += 1;
                queue.next_order
            });
        let retry_manager = manager.clone();
        let retry_runtime = runtime.clone();
        queue.entries.insert(
            key,
            Entry {
                order,
                resource: sailry_protocol::installation::ModelRef::Speech {
                    target: sailry_protocol::installation::Target::Controller,
                    id,
                }
                .resource(),
                status: Presentation::Starting,
                finished: false,
                connected: true,
                dismissing: false,
                retry: Some(Rc::new(move |attempt, window, cx| {
                    match start(&retry_manager, &retry_runtime, id, request, attempt) {
                        Ok(_) => {
                            controller(retry_manager.clone(), retry_runtime.clone(), window, cx)
                        }
                        Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
                    }
                })),
            },
        );
        queue.changed(cx);
        queue.open(window, cx);
    });
    let ticket = Ticket { queue, key };
    let origin = window.window_handle();
    let (sender, receive) = tokio::sync::oneshot::channel();
    cx.spawn(async move |cx| {
        let outcome = receive.await.unwrap_or_else(|_| {
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "installation result is unconfirmed",
            ))
        });
        cx.update(|cx| ticket.record_outcome(outcome, cx));
        if let Some(target) = cx.update(|cx| target(origin, cx)) {
            let _ = target.update(cx, |_, window, cx| ticket.close_empty(window, cx));
        }
    })
    .detach();
    sender
}

impl Queue {
    fn bind_controller(
        &mut self,
        manager: Arc<Manager>,
        runtime: Arc<tokio::runtime::Runtime>,
        cx: &mut Context<Self>,
    ) {
        if self
            .controller
            .as_ref()
            .is_some_and(|source| Arc::ptr_eq(&source.manager, &manager))
        {
            return;
        }
        let mut changes = manager.subscribe();
        let watch = cx.spawn(async move |queue, cx| {
            while changes.changed().await.is_ok() {
                if queue
                    .update(cx, |queue, cx| queue.sync_controller(cx))
                    .is_err()
                {
                    break;
                }
                let window = queue.update(cx, |queue, _| queue.window).ok().flatten();
                let handle = cx.update(|cx| {
                    cx.windows()
                        .into_iter()
                        .find(|handle| Some(handle.window_id()) == window)
                });
                if let Some(handle) = handle {
                    let _ = handle.update(cx, |_, window, cx| {
                        let _ = queue.update(cx, |queue, cx| queue.close_empty(window, cx));
                    });
                }
            }
        });
        self.controller = Some(Source {
            manager,
            runtime,
            _watch: watch,
        });
    }

    fn sync_controller(&mut self, cx: &mut Context<Self>) {
        let Some(source) = &self.controller else {
            return;
        };
        let manager = source.manager.clone();
        let runtime = source.runtime.clone();
        // Only the controller's own final native validation/index result removes
        // a successful task; a missing feed row does not establish an outcome.
        self.entries.retain(|key, _| {
            key.0 != Location::Controller || manager.outcome(key.1) != Some(Ok(()))
        });
        self.accept(
            Location::Controller,
            View {
                connected: true,
                tasks: manager.installations(),
                ..Default::default()
            },
        );
        for (key, entry) in &mut self.entries {
            if key.0 != Location::Controller {
                continue;
            }
            let Resource::Model {
                kind: Kind::Speech,
                id,
            } = &entry.resource
            else {
                continue;
            };
            let id: Option<ModelId> = if id == "sensevoice-int8" {
                None
            } else {
                let Ok(id) = id.parse() else {
                    continue;
                };
                Some(id)
            };
            let retry_manager = manager.clone();
            let retry_runtime = runtime.clone();
            let request = key.1;
            entry.retry = Some(Rc::new(move |attempt, window, cx| {
                match start(&retry_manager, &retry_runtime, id, request, attempt) {
                    Ok(_) => controller(retry_manager.clone(), retry_runtime.clone(), window, cx),
                    Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
                }
            }));
            if manager.outcome(key.1).is_some() {
                entry.finished = true;
            }
        }
        self.changed(cx);
    }

    pub(super) fn remove_controller(
        &mut self,
        key: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = &self.controller else {
            return;
        };
        match source.manager.dismiss(key.1) {
            Ok(()) => {
                self.entries.remove(&key);
                self.changed(cx);
                self.close_empty(window, cx);
            }
            Err(error) => crate::feedback::info("", tr(error).as_ref(), window, cx),
        }
    }
}

fn start(
    manager: &Arc<Manager>,
    runtime: &tokio::runtime::Runtime,
    id: Option<ModelId>,
    request: RequestId,
    attempt: Attempt,
) -> crate::dictation::Result<CancellationToken> {
    match attempt {
        Attempt::New => manager.start(id, None, runtime),
        Attempt::Recover => manager.start_request(id, None, request, runtime),
    }
}
