use super::*;

#[gpui::test]
fn visible_completion_stays_read_and_notifies(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    visual.update(|window, _| window.activate_window());
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, session);
        fixture.submit(index);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .summary()
                    .is_some_and(|session| session.activity.waiting.is_some())
            })
        });
        let notifications = visual.update(|window, cx| window.notifications(cx).len());
        fixture.answer(index);
        wait(visual, |cx| {
            shell
                .read(cx)
                .activity_snapshot(node)
                .is_some_and(|snapshot| {
                    snapshot.sessions.iter().any(|item| {
                        item.id == session.id
                            && item
                                .activity
                                .run
                                .as_ref()
                                .is_some_and(|run| run.status == Status::Completed)
                            && item.activity.attention.revision > 0
                            && !item.activity.attention.unread
                    })
                })
        });
        assert!(visual.update(|window, cx| window.notifications(cx).len()) > notifications);
        for prefix in ["live-session", "active-session"] {
            let selector = Box::leak(format!("{prefix}-{}-unread", session.id).into_boxed_str());
            assert!(visual.debug_bounds(selector).is_none());
        }
        let client = Client::new(fixture.nodes[index].local());
        let Output::Session(saved) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadSession {
                session: session.id,
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        assert!(
            !saved.activity.attention.unread,
            "read state reaches the execution node"
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

#[gpui::test]
fn completion_waits_for_open_and_tracks_other_controllers(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    visual.update(|window, _| window.activate_window());
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, session);
        fixture.submit(index);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .summary()
                    .is_some_and(|session| session.activity.waiting.is_some())
            })
        });
        open_background_session(&shell, visual, &fixture, index);
        fixture.answer(index);
        wait(visual, |cx| shell.read(cx).session_unread(node, session.id));
        let dot = format!("live-session-{}-unread", session.id);
        let selector: &'static str = Box::leak(dot.clone().into_boxed_str());
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(visual.debug_bounds(selector).is_some());
        // Opening a background completion acknowledges its shared attention revision.
        click(&shell, visual, dot);
        wait(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(selector).is_none());
        open_background_session(&shell, visual, &fixture, index);
        let client = Client::new(fixture.nodes[index].local());
        let Output::Session(current) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadSession {
                session: session.id,
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::Session(unread) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SetSessionRead {
                session: session.id,
                expected_revision: current.activity.attention.revision,
                read: false,
            })))
            .unwrap()
        else {
            panic!("session expected")
        };
        wait(visual, |cx| shell.read(cx).session_unread(node, session.id));
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SetSessionRead {
                session: session.id,
                expected_revision: unread.activity.attention.revision,
                read: true,
            })))
            .unwrap();
        wait(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(selector).is_none());
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

struct Background;

impl Render for Background {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn returning_to_window_or_conversation_reads_completion(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    visual.update(|window, _| window.activate_window());
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = &fixture.sessions[index];
        open(&shell, visual, &fixture, index, session);
        fixture.submit(index);
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx)
                    .summary()
                    .is_some_and(|session| session.activity.waiting.is_some())
            })
        });
        let background = if index == 0 {
            Some(visual.update(|_, cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    window.activate_window();
                    cx.new(|_| Background)
                })
                .unwrap()
            }))
        } else {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| shell.navigate(Page::Activity, window, cx))
            });
            None
        };
        fixture.answer(index);
        wait(visual, |cx| shell.read(cx).session_unread(node, session.id));
        if let Some(background) = background {
            visual.update(|window, _| window.activate_window());
            background
                .update(visual, |_, window, _| window.remove_window())
                .unwrap();
        } else {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.navigate(Page::Conversation, window, cx)
                })
            });
        }
        wait(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
