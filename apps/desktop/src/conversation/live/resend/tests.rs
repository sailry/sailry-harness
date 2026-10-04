use super::*;
use crate::conversation::live::tests::{fixture, support, wait};
use core::prelude::v1::test;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{ErrorCode, Fault, RequestId, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Observed {
    inner: Arc<dyn Transport>,
    lose: AtomicBool,
    requests: Mutex<Vec<(RequestId, sailry_protocol::conversation::Input)>>,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let submit = matches!(&request.command, Command::SubmitTurn { .. });
            if let Command::SubmitTurn { message, .. } = &request.command {
                self.requests
                    .lock()
                    .unwrap()
                    .push((request.id, message.clone()));
            }
            let admission = self.inner.dispatch(request).await?;
            if submit && self.lose.swap(false, Ordering::SeqCst) {
                admission.completion.await.unwrap().unwrap();
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected receipt loss",
                ));
            }
            Ok(admission)
        })
    }
}

#[gpui::test]
fn retries_original_input(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for lose in [false, true] {
            let fixture = fixture::Fixture::with_server(remote, |runtime| {
                runtime.block_on(support::Server::http(1, 400))
            });
            let original = sailry_protocol::conversation::Input {
                text: "Retry this original task".into(),
                attachments: vec![],
                references: vec![sailry_protocol::conversation::reference::Reference {
                    target: sailry_protocol::conversation::reference::Target::Project,
                    label: "Original project".into(),
                }],
            };
            fixture.execute(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: original.clone(),
            });
            let observed = Arc::new(Observed {
                inner: fixture.transport.clone(),
                lose: AtomicBool::new(lose),
                requests: Mutex::new(vec![]),
            });
            let mut binding = fixture.binding.clone();
            binding.client = Arc::new(Client::new(observed.clone()));
            let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
            wait(visual, |cx| {
                view.read(cx).connected()
                    && view.read(cx).history.snapshot.as_ref().is_some_and(|s| {
                        s.page
                            .runs
                            .first()
                            .is_some_and(|r| r.status == Status::Failed)
                    })
            });
            let turn = view.read_with(visual, |view, _| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                let turn = page.runs[0].turn;
                assert_eq!(messages::sent_input(page, turn), original);
                turn
            });
            let selector = format!("live-turn-retry-{turn}");
            assert!(view.read_with(visual, |view, _| view.can_resend(turn)));
            fixture::tap(visual, "live-chat-input");
            visual.simulate_input("Keep my unsent draft");
            view.update(visual, |view, _| {
                view.references.selected =
                    vec![sailry_protocol::conversation::reference::Reference {
                        target: sailry_protocol::conversation::reference::Target::Host,
                        label: "Unsent host".into(),
                    }];
            });
            fixture::tap(visual, &selector);
            if lose {
                wait(visual, |cx| {
                    view.read(cx).error.is_some() && !view.read(cx).pending
                });
                assert!(view.read_with(visual, |view, _| {
                    view.retry.as_ref().unwrap().is_resend(turn)
                }));
                fixture::tap(visual, &selector);
            }
            wait(visual, |cx| {
                let view = view.read(cx);
                !view.pending
                    && view.retry.is_none()
                    && view.history.snapshot.as_ref().is_some_and(|s| {
                        s.page.runs.len() == 2 && s.page.runs[1].status == Status::Completed
                    })
            });
            view.read_with(visual, |view, cx| {
                let page = &view.history.snapshot.as_ref().unwrap().page;
                assert_eq!(page.runs[0].turn, turn);
                assert_eq!(page.runs[0].status, Status::Failed);
                assert_eq!(messages::sent_input(page, page.runs[1].turn), original);
                assert_eq!(view.input.read(cx).value().as_ref(), "Keep my unsent draft");
                assert!(!view.can_resend(page.runs[1].turn));
                assert_eq!(view.references.selected.len(), 1);
                assert_eq!(view.references.selected[0].label, "Unsent host");
                assert!(view.error.is_none());
            });
            let requests = observed.requests.lock().unwrap();
            assert_eq!(requests.len(), if lose { 2 } else { 1 });
            assert!(
                requests
                    .iter()
                    .all(|(id, message)| *id == requests[0].0 && message == &original)
            );
            assert_eq!(fixture.task_requests(), 2);
            drop(requests);
            // Interrupted turns use the same affordance. Offline and active views cannot submit.
            view.update(visual, |view, cx| {
                let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
                let page = Arc::make_mut(&mut snapshot.page);
                page.runs[0].status = Status::Interrupted;
                assert!(view.can_resend(turn));
                view.history.connected = false;
                assert!(!view.can_resend(turn));
                view.history.connected = true;
                let page =
                    Arc::make_mut(&mut Arc::make_mut(view.history.snapshot.as_mut().unwrap()).page);
                page.runs[1].status = Status::Running;
                assert!(!view.can_resend(turn));
                cx.notify();
            });
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
