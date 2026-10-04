use super::*;
use crate::preferences::Preferences;
use sailry_protocol::terminal::Status;

struct Background;
impl Render for Background {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn preferences_and_shortcuts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let fixture = fixture::Fixture::new();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
        cx.set_global(Preferences::open(path.clone()));
        crate::preferences::update(cx, |data| {
            data.toast_seconds = 3;
            data.notifications[2] = false;
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let background = visual.update(|_, cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            window.activate_window();
            cx.new(|_| Background)
        })
        .unwrap()
    });
    wait(visual, |cx| {
        shell.read(cx).activity.observers.len() == 2
            && shell
                .read(cx)
                .activity
                .observers
                .values()
                .all(|o| o.view.connected)
    });
    fixture.submit(1);
    wait(visual, |cx| shell.read(cx).activity.inbox.unread() == 1);
    assert_eq!(visual.delivered_system_notifications().len(), 1);
    let notification = visual.delivered_system_notifications()[0].clone();
    assert_eq!(notification.body.as_ref(), "Review workspace changes 1");
    visual.simulate_system_notification_response(SystemNotificationResponse {
        tag: notification.tag,
        action_id: None,
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .current_chat()
            .is_some_and(|chat| chat.read(cx).session() == Some(fixture.sessions[1].id))
    });
    fixture.answer(1);
    wait(visual, |cx| {
        shell.read(cx).activity.inbox.notices().len() == 2
    });
    assert_eq!(visual.shown_system_notifications().len(), 1);
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        0
    );

    visual.update(|window, _| window.activate_window());
    fixture.submit(0);
    wait(visual, |cx| {
        shell.read(cx).activity.inbox.notices().len() == 3
    });
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        1
    );
    assert!(visual.debug_bounds("notification-open").is_some());
    assert!(visual.debug_bounds("notification-open").unwrap().size.width <= px(32.));
    let card = visual.debug_bounds("notification-card").unwrap();
    let open = visual.debug_bounds("notification-open").unwrap();
    let close = visual.debug_bounds("notification-close").unwrap();
    assert!(close.left() >= open.right() + px(4.));
    assert_eq!(card.right() - close.right(), px(12.));
    visual.executor().advance_clock(Duration::from_secs(4));
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(500));
    visual.run_until_parked();
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        0
    );

    visual.update(|_, cx| {
        crate::preferences::update(cx, |data| data.notifications[2] = true);
    });
    background
        .update(visual, |_, window, _| window.activate_window())
        .unwrap();
    fixture.answer(0);
    wait(visual, |cx| {
        shell.read(cx).activity.inbox.notices().len() == 4
    });
    let completed = visual
        .delivered_system_notifications()
        .last()
        .unwrap()
        .clone();
    assert_eq!(completed.title.as_ref(), tr("activity_completed").as_ref());
    assert_eq!(completed.body.as_ref(), "Review workspace changes 0");
    visual.update(|window, _| window.activate_window());

    for index in 0..2 {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let live = shell.live.as_mut().unwrap();
                live.select(fixture.nodes[index].id(), cx);
                live.project = fixture.sessions[index].project;
                shell.focus.focus(window, cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
        });
        let current = shell.read_with(visual, |shell, _| shell.page);
        for (key, page) in [
            ("secondary-5", current),
            ("secondary-3", current),
            ("secondary-4", current),
            ("secondary-,", Page::Settings),
        ] {
            let key = key.replace(
                "secondary",
                if cfg!(target_os = "macos") {
                    "cmd"
                } else {
                    "ctrl"
                },
            );
            visual.simulate_keystrokes(&key);
            visual.run_until_parked();
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.page),
                page,
                "{key}"
            );
        }
        assert_eq!(
            visual.update(|_, cx| crate::preferences::data(cx).toast_seconds),
            3
        );
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.open_live_terminals(window, cx))
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .selected_worktree()
                .is_some()
        });
        let registry = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .worktree_plugins(fixture.sessions[index].worktree, window, cx)
                    .unwrap()
            })
        });
        wait(visual, |cx| {
            registry
                .read(cx)
                .creation(sailry_protocol::plugin::desktop::ResourceKind::Terminal, cx)
                .is_some_and(|entry| entry.state.enabled)
        });
        click(visual, "plugin-control-commands-terminal-new");
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .any(|t| t.status == Status::Running)
        });
        let terminal = shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .find(|info| info.status == Status::Running)
                .unwrap()
                .id
        });
        let target = crate::panes::Target::Terminal(
            fixture.nodes[index].id(),
            fixture.sessions[index].worktree,
            terminal,
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                let state = shell.read(cx);
                state.page == Page::Terminal
                    && state.splits.read(cx).active == Some(target)
                    && state.plugin_panes.terminal(target, cx).is_some_and(|view| {
                        let view = view.read(cx);
                        view.info().is_some_and(|info| info.id == terminal)
                            && view.focus_handle(cx).is_focused(window)
                    })
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "terminal focus deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .all(|t| t.status == Status::Closed)
        });
    }
    assert_eq!(Preferences::open(path).data.toast_seconds, 3);
    background
        .update(visual, |_, window, _| window.remove_window())
        .unwrap();
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}
