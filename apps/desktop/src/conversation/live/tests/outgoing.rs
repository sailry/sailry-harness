use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use sailry_protocol::{Event, Fault, StreamId, Topic, Update, conversation::Change};

struct Delayed {
    inner: Arc<dyn Transport>,
    dispatch: CancellationToken,
    reply: CancellationToken,
    history: CancellationToken,
    identity: CancellationToken,
    held_input: Option<&'static str>,
    queue: Option<CancellationToken>,
    start: Option<CancellationToken>,
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
                held_input: self.held_input,
                queue: self.queue.clone(),
                start: self.start.clone(),
                admitted: false,
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
    held_input: Option<&'static str>,
    queue: Option<CancellationToken>,
    start: Option<CancellationToken>,
    admitted: bool,
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            let update = self.inner.next().await?;
            if let Update::ConversationFrame(frame) = &update {
                if matches!(&frame.change, Change::Queue(queue) if !queue.items.is_empty()) {
                    if let Some(queue) = &self.queue {
                        queue.cancelled().await;
                    }
                    self.admitted = true;
                }
                if self.admitted
                    && (matches!(&frame.change, Change::Queue(queue) if queue.items.is_empty())
                        || matches!(&frame.change, Change::Run(run) if matches!(run.status, Status::Running | Status::Stopping)))
                    && let Some(start) = &self.start
                {
                    start.cancelled().await;
                }
            }
            if matches!(&update, Update::Event(envelope) if matches!(envelope.event, Event::TurnQueued { .. }))
            {
                self.identity.cancelled().await;
            }
            let user = match &update {
                Update::ConversationFrame(frame) => {
                    matches!(&frame.change, Change::Entry(entry) if self.holds(entry))
                }
                Update::ConversationSnapshot(snapshot) => {
                    snapshot.page.entries.iter().any(|entry| self.holds(entry))
                }
                _ => false,
            };
            if user {
                self.history.cancelled().await;
            }
            Ok(update)
        })
    }
}

impl Events {
    fn holds(&self, entry: &sailry_protocol::conversation::Entry) -> bool {
        entry.author == "user"
            && self.held_input.is_none_or(|text| {
                entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Text(value) if value == text))
            })
    }
}

#[gpui::test]
fn idle_submission_keeps_transcript_placement(cx: &mut TestAppContext) {
    const MESSAGE: &str = "Keep the submitted input in this conversation";
    fixture::init(cx);
    for remote in [false, true] {
        for reply_first in [false, true] {
            let fixture = fixture::Fixture::with_tools(remote, vec![]);
            let queue = CancellationToken::new();
            let start = CancellationToken::new();
            let delayed = Arc::new(Delayed {
                inner: fixture.transport.clone(),
                dispatch: CancellationToken::new(),
                reply: CancellationToken::new(),
                history: CancellationToken::new(),
                identity: CancellationToken::new(),
                held_input: Some(MESSAGE),
                queue: Some(queue.clone()),
                start: Some(start.clone()),
            });
            let mut binding = fixture.binding.clone();
            binding.client = Arc::new(Client::new(delayed.clone()));
            let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
            wait(visual, |cx| {
                view.read(cx).connected() && view.read(cx).configured()
            });
            fixture::tap(visual, "live-chat-input");
            visual.simulate_input(MESSAGE);
            fixture::tap(visual, "live-chat-send");
            wait(visual, |cx| {
                view.read(cx).pending && view.read(cx).outgoing.is_some()
            });
            let (request, row) = view.read_with(visual, |view, _| {
                let outgoing = view.outgoing.as_ref().unwrap();
                assert!(!outgoing.queued);
                (outgoing.request.unwrap(), outgoing.row)
            });
            assert!(visual.debug_bounds("live-outgoing-message").is_some());
            assert!(visual.debug_bounds("queue-tray").is_none());
            delayed.dispatch.cancel();
            if reply_first {
                delayed.reply.cancel();
                wait(visual, |cx| {
                    !view.read(cx).pending
                        && view
                            .read(cx)
                            .outgoing
                            .as_ref()
                            .is_some_and(|message| message.turn.is_some())
                });
                assert!(view.read_with(visual, |view, _| {
                    view.history
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .page
                        .queue
                        .items
                        .is_empty()
                }));
                assert!(visual.debug_bounds("live-outgoing-message").is_some());
            }
            queue.cancel();
            wait(visual, |cx| view.read(cx).transcript_pending().is_some());
            let turn = view.read_with(visual, |view, _| {
                let outgoing = view.outgoing.as_ref().unwrap();
                let queued = view.transcript_pending().unwrap();
                assert_eq!(queued.request, Some(request));
                assert_eq!(outgoing.request, Some(request));
                assert_eq!(outgoing.row, row);
                assert_eq!(outgoing.turn, Some(queued.turn));
                assert!(!outgoing.queued);
                assert_eq!(view.pending, !reply_first);
                queued.turn
            });
            assert!(visual.debug_bounds("live-outgoing-message").is_some());
            assert!(visual.debug_bounds("live-outgoing-queued").is_some());
            assert!(visual.debug_bounds("queue-tray").is_none());
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("queue-row-{turn}").into_boxed_str()))
                    .is_none()
            );

            // Identical previews without this exact command identity stay visible in the queue.
            visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
                    let page = Arc::make_mut(&mut snapshot.page);
                    let mut foreign = page.queue.items[0].clone();
                    foreign.turn = TurnId::new();
                    foreign.request = Some(sailry_protocol::RequestId::new());
                    let mut historical = page.queue.items[0].clone();
                    historical.turn = TurnId::new();
                    historical.request = None;
                    page.queue.items.insert(0, historical);
                    page.queue.items.push(foreign);
                    cx.notify();
                })
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("queue-tray").is_some());
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("queue-row-{turn}").into_boxed_str()))
                    .is_none()
            );
            let (historical, foreign) = view.read_with(visual, |view, _| {
                let items = &view.history.snapshot.as_ref().unwrap().page.queue.items;
                (items[0].turn, items[2].turn)
            });
            assert!(
                visual
                    .debug_bounds(Box::leak(
                        format!("queue-row-{historical}").into_boxed_str()
                    ))
                    .is_some()
            );
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("queue-row-{foreign}").into_boxed_str()))
                    .is_some()
            );
            visual.update(|_, cx| {
                view.update(cx, |view, cx| {
                    let snapshot = Arc::make_mut(view.history.snapshot.as_mut().unwrap());
                    Arc::make_mut(&mut snapshot.page)
                        .queue
                        .items
                        .retain(|item| item.turn == turn);
                    cx.notify();
                })
            });
            delayed.identity.cancel();
            delayed.reply.cancel();
            start.cancel();
            wait(visual, |cx| {
                view.read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn && run.status == Status::Running)
            });
            assert!(visual.debug_bounds("live-outgoing-queued").is_none());
            assert!(visual.debug_bounds("queue-tray").is_none());
            assert!(
                visual
                    .debug_bounds(Box::leak(format!("live-user-text-{turn}").into_boxed_str()))
                    .is_some()
            );
            delayed.history.cancel();
            wait(visual, |cx| {
                view.read(cx).outgoing.is_none()
                    && !view.read(cx).pending
                    && view
                        .read(cx)
                        .history
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .page
                        .runs
                        .iter()
                        .any(|run| run.turn == turn && run.status == Status::Completed)
            });
            assert_eq!(fixture.task_requests(), 1);
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
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
                held_input: None,
                queue: None,
                start: None,
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
            visual.update(|window, cx| {
                use gpui_kit::test::TestWindowExt;
                window.render_frame(cx);
                let button = window.find("live-send");
                assert!(button.visible());
                assert_ne!(button.disabled(), Some(true));
                window.click("live-send", cx);
            });
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
            view.read_with(visual, |view, cx| {
                assert!(view.pending);
                assert!(view.input.read(cx).value().is_empty());
                assert_eq!(view.rows.len(), 1);
                assert!(
                    view.history
                        .snapshot
                        .as_ref()
                        .is_none_or(|s| s.page.entries.is_empty())
                );
            });
            // A new draft equal to the sent text is still a different draft.
            fixture::tap(visual, "live-chat-input");
            visual.simulate_input("Show this before the request returns");
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
            view.read_with(visual, |view, cx| {
                assert_eq!(
                    view.input.read(cx).value(),
                    "Show this before the request returns"
                );
                assert!(view.submissions.active.is_none());
            });
            assert_eq!(fixture.task_requests(), 1);
            visual.update(|window, _| window.remove_window());
            drop(view);
            fixture.close();
        }
    }
}

#[gpui::test]
fn guidance_handoff_precedes_canonical_history(cx: &mut TestAppContext) {
    const MESSAGE: &str = "Use the settled tool result and finish briefly 中文 🙂";
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                serde_json::json!({"access":"unrestricted","command":"printf ready"}),
            )],
        );
        fixture.start();
        let delayed = Arc::new(Delayed {
            inner: fixture.transport.clone(),
            dispatch: CancellationToken::new(),
            reply: CancellationToken::new(),
            history: CancellationToken::new(),
            identity: CancellationToken::new(),
            held_input: Some(MESSAGE),
            queue: None,
            start: None,
        });
        delayed.dispatch.cancel();
        delayed.reply.cancel();
        delayed.identity.cancel();
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(delayed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).history.calls.first().is_some_and(|call| {
                call.approval.as_ref().is_some_and(|approval| {
                    approval.state == sailry_protocol::conversation::ApprovalState::Pending
                })
            })
        });
        let (active, approval) = view.read_with(visual, |view, _| {
            let call = &view.history.calls[0];
            (call.turn, call.approval.as_ref().unwrap().id)
        });
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input(MESSAGE);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).sent.len() == 1 && !view.read(cx).pending
        });
        let turn = view.read_with(visual, |view, _| {
            view.sent.values().next().unwrap().turn.unwrap()
        });
        let guide: &'static str = Box::leak(format!("live-queue-guide-{turn}").into_boxed_str());
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .queue
                        .items
                        .iter()
                        .any(|item| item.turn == turn)
                })
        });
        fixture::tap(visual, guide);
        wait(visual, |cx| {
            view.read(cx)
                .outgoing
                .as_ref()
                .is_some_and(|message| message.turn == Some(turn) && message.guidance.is_some())
        });
        view.read_with(visual, |view, _| {
            let snapshot = view.history.snapshot.as_ref().unwrap();
            assert!(
                snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| { run.turn == active && run.status == Status::Running })
            );
            let queued = snapshot
                .page
                .runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap();
            assert_eq!(queued.status, Status::Queued);
            assert!(queued.input.is_none());
        });
        let phase: &'static str =
            Box::leak(format!("live-turn-phase-{turn}-turn_awaiting_response").into_boxed_str());
        assert!(visual.debug_bounds("live-outgoing-message").is_some());
        assert!(visual.debug_bounds(phase).is_none());
        fixture.execute(Command::ResolveApproval {
            session: fixture.session.id,
            approval,
            decision: sailry_protocol::conversation::Decision::Approve,
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.page.runs.iter().any(|run| {
                        run.turn == turn && run.status == Status::Running && run.input.is_some()
                    })
                })
        });
        view.read_with(visual, |view, _| {
            let snapshot = view.history.snapshot.as_ref().unwrap();
            assert!(
                snapshot
                    .page
                    .runs
                    .iter()
                    .any(|run| { run.turn == active && run.status == Status::Continued })
            );
            assert!(!snapshot.page.entries.iter().any(|entry| entry.turn == turn));
            let message = view.outgoing.as_ref().unwrap();
            assert_eq!(message.turn, Some(turn));
            assert_eq!(message.input.text, MESSAGE);
            assert!(message.guidance.is_some());
        });
        let user: &'static str = Box::leak(format!("live-user-text-{turn}").into_boxed_str());
        let user_bounds = visual.debug_bounds(user).unwrap();
        let phase_bounds = visual.debug_bounds(phase).unwrap();
        assert!(user_bounds.bottom() <= phase_bounds.top());
        assert!(visual.debug_bounds("live-outgoing-message").is_none());
        delayed.history.cancel();
        wait(visual, |cx| view.read(cx).outgoing.is_none());
        view.read_with(visual, |view, _| {
            assert_eq!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .filter(|entry| entry.turn == turn && entry.author == "user")
                    .count(),
                1,
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn queued_prompt_survives_until_canonical_history(cx: &mut TestAppContext) {
    const MESSAGE: &str = concat!(
        "Run this queued message and preserve its full input while execution starts. ",
        "The queue preview is deliberately shorter than this prompt, so the running ",
        "conversation must retain the original text without replacing it with the preview. ",
        "A second queued message must leave this first message intact until canonical history arrives."
    );
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                serde_json::json!({"access":"unrestricted","command":"printf ready"}),
            )],
        );
        fixture.start();
        let delayed = Arc::new(Delayed {
            inner: fixture.transport.clone(),
            dispatch: CancellationToken::new(),
            reply: CancellationToken::new(),
            history: CancellationToken::new(),
            identity: CancellationToken::new(),
            held_input: Some(MESSAGE),
            queue: None,
            start: None,
        });
        delayed.dispatch.cancel();
        delayed.reply.cancel();
        delayed.identity.cancel();
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(delayed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx).history.calls.first().is_some_and(|call| {
                call.approval.as_ref().is_some_and(|approval| {
                    approval.state == sailry_protocol::conversation::ApprovalState::Pending
                })
            })
        });
        let approval = view.read_with(visual, |view, _| {
            view.history.calls[0].approval.as_ref().unwrap().id
        });
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input(MESSAGE);
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).sent.len() == 1
                && view.read(cx).outgoing.is_none()
                && !view.read(cx).pending
        });
        let turn = view.read_with(visual, |view, _| {
            let message = view.sent.values().next().unwrap();
            assert!(message.queued);
            assert_eq!(message.input.text, MESSAGE);
            let turn = message.turn.unwrap();
            let queued = view
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .queue
                .items
                .iter()
                .find(|item| item.turn == turn)
                .unwrap();
            assert!(queued.truncated);
            assert_ne!(queued.preview, MESSAGE);
            turn
        });
        fixture::tap(visual, "live-chat-input");
        visual.simulate_input("Second queued message");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).sent.len() == 2 && !view.read(cx).pending
        });
        view.read_with(visual, |view, _| {
            assert!(view.outgoing.is_none());
            assert_eq!(view.sent.get(&turn).unwrap().input.text, MESSAGE);
            assert!(
                view.sent
                    .values()
                    .any(|message| message.input.text == "Second queued message")
            );
        });
        fixture.execute(Command::ResolveApproval {
            session: fixture.session.id,
            approval,
            decision: sailry_protocol::conversation::Decision::Approve,
        });
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
                        .any(|run| run.turn == turn && run.status == Status::Running)
                })
        });
        view.read_with(visual, |view, _| {
            assert_eq!(view.sent.get(&turn).unwrap().input.text, MESSAGE);
            assert!(
                !view
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .any(|entry| entry.turn == turn && entry.author == "user")
            );
        });
        let user = visual
            .debug_bounds(Box::leak(format!("live-user-text-{turn}").into_boxed_str()))
            .expect("queued prompt appears as execution starts");
        let phase = visual
            .debug_bounds(Box::leak(
                format!("live-turn-phase-{turn}-turn_awaiting_response").into_boxed_str(),
            ))
            .unwrap();
        assert!(user.bottom() <= phase.top());
        delayed.history.cancel();
        wait(visual, |cx| {
            view.read(cx).outgoing.is_none() && !view.read(cx).sent.contains_key(&turn)
        });
        assert!(
            visual
                .debug_bounds(Box::leak(format!("live-user-text-{turn}").into_boxed_str()))
                .is_some()
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
