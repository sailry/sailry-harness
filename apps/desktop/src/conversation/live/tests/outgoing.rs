use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use sailry_protocol::{Event, Fault, StreamId, Topic, Update, conversation::Change};

struct Delayed {
    inner: Arc<dyn Transport>,
    dispatch: CancellationToken,
    reply: CancellationToken,
    history: CancellationToken,
    identity: CancellationToken,
}

impl Transport for Delayed {
    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        self.inner.open(stream)
    }
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let inner = self.inner.subscribe(topic).await?;
            Ok(Box::new(Events {
                inner,
                history: self.history.clone(),
                identity: self.identity.clone(),
            }) as Box<dyn Subscription>)
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let send = matches!(request.command, Command::SubmitTurn { .. });
            if send || matches!(request.command, Command::CreateSession { .. }) {
                self.dispatch.cancelled().await;
            }
            let admission = self.inner.dispatch(request).await?;
            if !send {
                return Ok(admission);
            }
            let receipt = admission.receipt;
            let result = admission.completion.await.unwrap();
            self.reply.cancelled().await;
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(result).unwrap();
            Ok(Admission {
                receipt,
                completion,
            })
        })
    }
}

struct Events {
    inner: Box<dyn Subscription>,
    history: CancellationToken,
    identity: CancellationToken,
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            let update = self.inner.next().await?;
            if matches!(&update, Update::Event(envelope) if matches!(envelope.event, Event::TurnQueued { .. }))
            {
                self.identity.cancelled().await;
            }
            let user = match &update {
                Update::ConversationFrame(frame) => {
                    matches!(&frame.change, Change::Entry(entry) if entry.author == "user")
                }
                Update::ConversationSnapshot(snapshot) => snapshot
                    .page
                    .entries
                    .iter()
                    .any(|entry| entry.author == "user"),
                _ => false,
            };
            if user {
                self.history.cancelled().await;
            }
            Ok(update)
        })
    }
}

#[gpui::test]
fn shows_before_admission_and_reconciles_before_reply(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        for existing in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
                panic!("snapshot expected");
            };
            let mut provider = snapshot.providers[0].clone();
            provider.models[0].vision = true;
            fixture.execute(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            });
            let delayed = Arc::new(Delayed {
                inner: fixture.transport.clone(),
                dispatch: CancellationToken::new(),
                reply: CancellationToken::new(),
                history: CancellationToken::new(),
                identity: CancellationToken::new(),
            });
            let mut binding = fixture.binding.clone();
            binding.client = Arc::new(Client::new(delayed.clone()));
            let (view, visual) =
                fixture::open_session(cx, binding, existing.then(|| fixture.session.clone()));
            // Keep both the welcome composer and the outgoing image in the viewport.
            let window = visual.update(|window, _| window.window_handle());
            visual.simulate_window_resize(window, size(px(1100.), px(1200.)));
            wait(visual, |cx| view.read(cx).connected());
            visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    view.config = Some(fixture.session.config.clone());
                    cx.notify();
                })
            });
            let path = fixture.directory.path().join("attachment.png");
            image::DynamicImage::new_rgba8(8, 4).save(&path).unwrap();
            visual.update(|window, cx| {
                view.update(cx, |view, cx| view.attach_paths(vec![path], window, cx))
            });
            wait(visual, |cx| {
                let view = view.read(cx);
                view.has_attachments()
                    && view.attachments.sendable()
                    && attachments::images::Images::aspect_ratio(
                        &view.images,
                        &view.attachment_previews()[0],
                        cx,
                    ) == 2.
            });
            fixture::tap(visual, "live-chat-input");
            visual.simulate_input("Show this before the request returns");
            fixture::tap(visual, "live-chat-send");
            wait(visual, |cx| view.read(cx).outgoing.is_some());
            assert!(visual.debug_bounds("live-outgoing-message").is_some());
            let image_selector = view.read_with(visual, |view, _| {
                format!(
                    "image-card-{}",
                    view.outgoing.as_ref().unwrap().attachments[0].id()
                )
            });
            let image_selector: &'static str = Box::leak(image_selector.into_boxed_str());
            assert!(visual.debug_bounds(image_selector).is_some());
            assert!(fixture.server.requests.lock().unwrap().is_empty());
            // A new-session image is uploaded before the held CreateSession command.
            wait(visual, |cx| view.read(cx).pending);
            view.read_with(visual, |view, _| {
                assert!(view.pending);
                assert_eq!(view.rows.len(), 1);
                assert!(
                    view.history
                        .snapshot
                        .as_ref()
                        .is_none_or(|s| s.page.entries.is_empty())
                );
            });
            delayed.dispatch.cancel();
            if existing {
                wait(visual, |cx| {
                    view.read(cx)
                        .history
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| {
                            snapshot
                                .page
                                .runs
                                .iter()
                                .any(|run| run.status == Status::Running)
                        })
                });
                view.read_with(visual, |view, _| {
                    assert!(view.outgoing.as_ref().unwrap().turn.is_none());
                    assert_eq!(
                        view.rows.len(),
                        1,
                        "only the draft before admission identity"
                    );
                });
                assert!(visual.debug_bounds("live-outgoing-message").is_some());
                assert!(visual.debug_bounds(image_selector).is_some());
            }
            delayed.identity.cancel();
            if existing {
                wait(visual, |cx| {
                    let view = view.read(cx);
                    view.outgoing.as_ref().is_some_and(|outgoing| {
                        view.history.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.page.runs.iter().any(|run| {
                                Some(run.turn) == outgoing.turn && run.status == Status::Running
                            })
                        })
                    })
                });
                let turn = view.read_with(visual, |view, _| {
                    assert_eq!(view.rows.len(), 1, "one row during admission");
                    assert!(
                        view.history
                            .snapshot
                            .as_ref()
                            .unwrap()
                            .page
                            .entries
                            .is_empty()
                    );
                    view.outgoing.as_ref().unwrap().turn.unwrap()
                });
                let user = visual
                    .debug_bounds(Box::leak(format!("live-user-text-{turn}").into_boxed_str()))
                    .unwrap();
                let phase = visual
                    .debug_bounds(Box::leak(
                        format!("live-turn-phase-{turn}-turn_awaiting_response").into_boxed_str(),
                    ))
                    .unwrap();
                assert!(
                    user.bottom() <= phase.top(),
                    "the prompt precedes the response timer"
                );
                assert!(visual.debug_bounds(image_selector).is_some());
                assert!(visual.debug_bounds("live-outgoing-message").is_none());
            }
            delayed.history.cancel();
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.page.runs.iter().any(|r| r.status == Status::Completed))
            });
            wait(visual, |cx| view.read(cx).outgoing.is_none());
            assert!(visual.debug_bounds("live-outgoing-message").is_none());
            view.read_with(visual, |view, _| {
                assert!(view.pending, "the server reply is still held");
                assert_eq!(view.rows.len(), 1, "only the canonical turn remains");
            });
            delayed.reply.cancel();
            wait(visual, |cx| !view.read(cx).pending);
            assert_eq!(fixture.task_requests(), 1);
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}
