use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use sailry_protocol::{ErrorCode, Fault, StreamId, Topic};
use std::sync::Mutex;

#[derive(Clone, Copy)]
enum Failure {
    Submit,
    Create,
    Upload,
    Lost,
}

struct Held {
    inner: Arc<dyn Transport>,
    release: CancellationToken,
    failure: Failure,
    requests: Mutex<Vec<Request>>,
}

impl Transport for Held {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.inner.open(stream)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let held = match self.failure {
                Failure::Submit | Failure::Lost => {
                    matches!(request.command, Command::SubmitTurn { .. })
                }
                Failure::Create => matches!(request.command, Command::CreateSession { .. }),
                Failure::Upload => matches!(request.command, Command::UploadAttachment(_)),
            };
            if !held {
                return self.inner.dispatch(request).await;
            }
            let first = {
                let mut requests = self.requests.lock().unwrap();
                requests.push(request.clone());
                requests.len() == 1
            };
            self.release.cancelled().await;
            if !matches!(self.failure, Failure::Lost) {
                return Err(Fault::invalid("injected submission failure"));
            }
            let admission = self.inner.dispatch(request).await?;
            if !first {
                return Ok(admission);
            }
            admission.completion.await.unwrap().unwrap();
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "injected missing submission response",
            ))
        })
    }
}

fn transport(fixture: &fixture::Fixture, failure: Failure) -> Arc<Held> {
    Arc::new(Held {
        inner: fixture.transport.clone(),
        release: CancellationToken::new(),
        failure,
        requests: Mutex::new(vec![]),
    })
}

#[gpui::test]
fn restores_rich_input_after_definite_failure(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for existing in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let held = transport(
                &fixture,
                if existing {
                    Failure::Submit
                } else {
                    Failure::Create
                },
            );
            let mut binding = fixture.binding.clone();
            binding.client = Arc::new(Client::new(held.clone()));
            let (view, visual) =
                fixture::open_session(cx, binding, existing.then(|| fixture.session.clone()));
            wait(visual, |cx| view.read(cx).connected());
            let path = fixture.directory.path().join("attachment.txt");
            std::fs::write(&path, "Retained attachment").unwrap();
            visual.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.config = Some(fixture.session.config.clone());
                    view.attach_paths(vec![path.clone()], window, cx);
                    view.input.update(cx, |input, cx| {
                        input.set_value("Keep this input\n", window, cx)
                    });
                    view.insert_file_references(vec![("source.txt".into(), false)], window, cx);
                    view.input
                        .update(cx, |input, cx| input.set_selected_range(5..9, cx));
                })
            });
            wait(visual, |cx| {
                let view = view.read(cx);
                view.has_attachments()
                    && if existing {
                        view.attachments.ready()
                    } else {
                        view.attachments.sendable()
                    }
            });
            let original = view.read_with(visual, |view, cx| {
                (
                    view.input.read(cx).content(),
                    view.input.read(cx).selected_range(),
                    view.references.selected.clone(),
                    view.attachments.ids(),
                )
            });
            visual.update(|window, cx| view.update(cx, |view, cx| view.send(window, cx)));
            wait(visual, |cx| {
                view.read(cx).pending && !held.requests.lock().unwrap().is_empty()
            });
            let prepared = view.read_with(visual, |view, cx| {
                assert!(view.input.read(cx).value().is_empty());
                assert!(view.references.selected.is_empty());
                assert!(view.submissions.active.is_some());
                assert!(view.attachments.ready());
                let ids = view.attachments.ids();
                assert_eq!(ids.len(), 1);
                if existing {
                    assert_eq!(ids, original.3);
                    let requests = held.requests.lock().unwrap();
                    let Command::SubmitTurn { message, .. } = &requests[0].command else {
                        panic!("submission expected")
                    };
                    assert_eq!(message.attachments, ids);
                } else {
                    assert!(original.3.is_empty());
                    assert!(matches!(
                        held.requests.lock().unwrap()[0].command,
                        Command::CreateSession { .. }
                    ));
                }
                ids
            });
            held.release.cancel();
            wait(visual, |cx| !view.read(cx).pending);
            view.read_with(visual, |view, cx| {
                let input = view.input.read(cx);
                assert_eq!(input.value().as_ref(), original.0.text().as_ref());
                assert_eq!(input.content().tokens(), original.0.tokens());
                assert_eq!(input.selected_range(), original.1);
                assert_eq!(view.references.selected, original.2);
                assert_eq!(view.attachments.ids(), prepared);
                if existing {
                    assert_eq!(view.attachments.ids(), original.3);
                }
                assert!(view.has_attachments());
                let sources = view.attachment_sources();
                assert_eq!(sources.len(), 1);
                assert!(matches!(&sources[0], attachments::images::Local::Path(source) if source == &path));
                assert_eq!(sources[0].name().as_deref(), Some("attachment.txt"));
                assert!(view.submissions.active.is_none());
                assert!(view.submissions.failed.is_empty());
                assert!(view.retry.is_none());
                assert!(view.outgoing.is_none());
            });
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "Retained attachment"
            );
            assert_eq!(held.requests.lock().unwrap().len(), 1);
            assert_eq!(fixture.task_requests(), 0);
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}

#[gpui::test]
fn keeps_new_input_and_failed_capture(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let held = transport(&fixture, Failure::Submit);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(held.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("Original input");
        fixture::tap(visual, "live-chat-send");
        wait(visual, |cx| view.read(cx).pending);
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("New input");
        held.release.cancel();
        wait(visual, |cx| !view.read(cx).pending);
        let id = view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "New input");
            assert_eq!(view.submissions.failed.len(), 1);
            *view.submissions.failed.keys().next().unwrap()
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(!view.restore_submission(id, window, cx));
                assert!(view.submissions.failed.contains_key(&id));
                view.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                assert!(view.restore_submission(id, window, cx));
            })
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Original input");
            assert!(view.submissions.failed.is_empty());
        });
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn retains_original_request_after_unknown_receipt(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let held = transport(&fixture, Failure::Lost);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(held.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("One admitted turn");
        fixture::tap(visual, "live-chat-send");
        wait(visual, |cx| view.read(cx).pending);
        let id = view.read_with(visual, |view, _| {
            view.submissions.active.as_ref().unwrap().id
        });
        held.release.cancel();
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).error.is_some()
        });
        assert_eq!(
            held.requests.lock().unwrap().len(),
            1,
            "no automatic replay"
        );
        view.read_with(visual, |view, cx| {
            assert!(view.retry.is_some());
            assert_eq!(held.requests.lock().unwrap()[0].id, id);
            assert_eq!(view.submissions.active.as_ref().unwrap().id, id);
            assert!(view.input.read(cx).value().is_empty());
            assert!(view.submissions.failed.is_empty());
        });
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("New input");
        visual.update(|window, cx| view.update(cx, |view, cx| view.retry(window, cx)));
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).retry.is_none()
        });
        wait(visual, |_| fixture.task_requests() > 0);
        let requests = held.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(fixture.task_requests(), 1);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "New input"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn restores_after_staged_upload_failure(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let held = transport(&fixture, Failure::Upload);
        let client = Arc::new(Client::new(held.clone()));
        let binding = Binding {
            client: client.clone(),
            defaults: client,
            project: None,
            worktree: None,
            ..fixture.binding.clone()
        };
        let (view, visual) = fixture::open_session(cx, binding, None);
        wait(visual, |cx| {
            view.read(cx).connected() && view.read(cx).configured()
        });
        let path = fixture.directory.path().join("attachment.txt");
        std::fs::write(&path, "Staged input").unwrap();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.attach_paths(vec![path], window, cx);
                view.input.update(cx, |input, cx| {
                    input.set_value("Retain this upload", window, cx)
                });
                view.send(window, cx);
            })
        });
        wait(visual, |_| !held.requests.lock().unwrap().is_empty());
        view.read_with(visual, |view, cx| {
            assert!(view.input.read(cx).value().is_empty())
        });
        held.release.cancel();
        wait(visual, |cx| !view.read(cx).busy());
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "Retain this upload");
            assert!(view.has_attachments());
            assert!(view.submissions.active.is_none());
            assert!(view.submissions.failed.is_empty());
            assert!(view.outgoing.is_none());
        });
        assert_eq!(fixture.task_requests(), 0);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
