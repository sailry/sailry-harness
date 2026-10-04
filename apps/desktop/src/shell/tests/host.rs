use super::workspace::click;
use super::*;
use crate::conversation::{
    fixture,
    turn::{Phase, Status},
};

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

fn bottom(cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(600.), px(400.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
        ..Default::default()
    });
    redraw(cx);
}

#[gpui::test]
fn background_preview_completion(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations[&(0, 0)].input.update(cx, |input, cx| {
                input.set_value("Preview task", window, cx);
                input.focus(window, cx);
            });
        })
    });
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(0, window, cx)));
    redraw(&mut cx);
    bottom(&mut cx);
    assert!(cx.debug_bounds("host-activity-0-0").is_some());
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(10));
    redraw(&mut cx);
    bottom(&mut cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Host);
    assert!(cx.debug_bounds("host-activity-0-0").is_none());
    assert!(cx.debug_bounds("host-activity-empty").is_some());
}

#[gpui::test]
fn activity_navigation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            for key in [(0, 0), (1, 0)] {
                shell.select_session(key, window, cx);
                shell.conversations.get_mut(&key).unwrap().turns = vec![fixture::sample(3)];
                shell.conversations[&key]
                    .input
                    .update(cx, |input, cx| input.set_value("Kept draft", window, cx));
            }
            shell.select_host(0, window, cx);
        })
    });
    redraw(&mut cx);
    bottom(&mut cx);
    assert!(cx.debug_bounds("host-runtime-facts").is_some());
    assert!(cx.debug_bounds("host-activity-0-0").is_some());
    assert!(cx.debug_bounds("host-activity-1-0").is_none());
    click(&mut cx, "host-activity-0-0");
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!((shell.host, shell.session), (0, 0));
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Kept draft"
        );
    });
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_host(1, window, cx)));
    redraw(&mut cx);
    bottom(&mut cx);
    assert!(cx.debug_bounds("host-activity-1-0").is_some());
    assert!(cx.debug_bounds("host-activity-0-0").is_none());
    click(&mut cx, "host-activity-1-0");
    assert_eq!(
        cx.update(|_, cx| (shell.read(cx).host, shell.read(cx).session)),
        (1, 0)
    );
}

#[gpui::test]
fn activity_retirement(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for status in [
        Status::Queued,
        Status::Running(Phase::Thinking),
        Status::Waiting,
        Status::Completed,
        Status::Failed,
        Status::Cancelled,
    ] {
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.conversations.get_mut(&(0, 0)).unwrap().turns[0].status = status;
                shell.select_host(0, window, cx);
            })
        });
        redraw(&mut cx);
        bottom(&mut cx);
        assert_eq!(
            cx.debug_bounds("host-activity-0-0").is_some(),
            status.active()
        );
        assert_eq!(
            cx.debug_bounds("host-activity-empty").is_some(),
            !status.active()
        );
        assert_eq!(
            cx.update(|_, cx| shell.read(cx).workspace.terminals.len()),
            2
        );
    }
}

#[gpui::test]
fn unavailable_activity_owners(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations.get_mut(&(0, 0)).unwrap().turns = vec![fixture::sample(3)];
            shell.workspace.sessions.get_mut(&(0, 0)).unwrap().archived = true;
            shell.select_host(0, window, cx);
        })
    });
    redraw(&mut cx);
    bottom(&mut cx);
    click(&mut cx, "host-activity-0-0");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .workspace
                .sessions
                .get_mut(&(0, 0))
                .unwrap()
                .owner
                .worktree = 99;
            shell.select_host(0, window, cx);
        })
    });
    redraw(&mut cx);
    bottom(&mut cx);
    assert!(cx.debug_bounds("host-activity-0-0").is_none());
}

#[gpui::test]
fn long_activity_bounds(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let handle = cx.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            shell.update(cx, |shell, cx| {
                shell.conversations.get_mut(&(0, 0)).unwrap().turns = vec![fixture::sample(3)];
                shell.workspace.sessions.get_mut(&(0, 0)).unwrap().title =
                    "Long session title ".repeat(30).into();
                shell.workspace.projects.get_mut(&0).unwrap().name =
                    "Long project name ".repeat(30).into();
                shell.select_host(0, window, cx);
            });
            window.window_handle()
        });
        cx.simulate_window_resize(handle, size(px(760.), px(560.)));
        redraw(&mut cx);
        bottom(&mut cx);
        let row = cx.debug_bounds("host-activity-0-0").unwrap();
        let status = cx.debug_bounds("host-activity-status-0-0").unwrap();
        assert!(row.right() <= px(760.));
        assert!(row.bottom() <= px(560.));
        assert!(status.right() <= row.right());
        assert!(status.left() >= row.left());
        click(&mut cx, "host-activity-0-0");
        assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
    }
}
