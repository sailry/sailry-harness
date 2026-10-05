use super::*;
use sailry_client::activity::{Lane, lane};
use sailry_protocol::{
    Delegation, TurnId,
    activity::Waiting,
    conversation::{Run, RunKind},
};

fn selector(value: impl Into<String>) -> &'static str {
    Box::leak(value.into().into_boxed_str())
}

fn avatar(node: NodeId, session: SessionId) -> &'static str {
    selector(format!("header-session-{node:?}-{session}"))
}

fn glyph(state: &str, session: SessionId) -> &'static str {
    selector(format!("header-avatar-{state}-{session}"))
}

fn hover(visual: &mut VisualTestContext, target: &'static str) {
    let bounds = visual.debug_bounds(target).unwrap();
    visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn opens_local_and_remote_attention(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
    assert!(visual.debug_bounds("header-activity").is_none());
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let session = &fixture.sessions[index];
        fixture.submit(index);
        wait(visual, |cx| {
            shell
                .read(cx)
                .active_sessions()
                .iter()
                .any(|(n, s)| *n == node && s.id == session.id && lane(s) == Lane::Waiting)
        });
        let item = avatar(node, session.id);
        assert!(visual.debug_bounds("header-activity_waiting").is_some());
        assert!(visual.debug_bounds("header-activity_running").is_none());
        assert!(visual.debug_bounds("header-activity_completed").is_none());
        assert!(visual.debug_bounds(glyph("waiting", session.id)).is_some());
        assert!(visual.debug_bounds(glyph("running", session.id)).is_none());
        let resting = visual.debug_bounds(item).unwrap();
        assert_eq!(resting.size, size(px(28.), px(28.)));
        assert!(resting.right() <= visual.debug_bounds("header-actions").unwrap().left());
        hover(visual, item);
        assert_eq!(visual.debug_bounds(item).unwrap().top(), resting.top());
        show_preview(visual, item);
        hover(visual, "resource-title");
        visual.executor().advance_clock(Duration::from_millis(350));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds(selector(format!("{item}-preview")))
                .is_none()
        );
        assert_eq!(visual.debug_bounds(item).unwrap().top(), resting.top());
        show_preview(visual, item);
        click(&shell, visual, item.into());
        wait(visual, |cx| {
            shell.read(cx).session_scope.active == Key::Session(node, session.id)
        });
        assert!(
            visual
                .debug_bounds(selector(format!("live-session-{}", session.id)))
                .is_some()
        );
        open(
            &shell,
            visual,
            &fixture,
            1 - index,
            &fixture.sessions[1 - index],
        );
        fixture.answer(index);
        wait(visual, |cx| {
            shell
                .read(cx)
                .active_sessions()
                .iter()
                .any(|(n, s)| *n == node && s.id == session.id && lane(s) == Lane::Completed)
        });
        assert!(visual.debug_bounds("header-activity_waiting").is_none());
        assert!(visual.debug_bounds("header-activity_completed").is_some());
        assert!(
            visual
                .debug_bounds(glyph("completed", session.id))
                .is_some()
        );
        assert!(visual.debug_bounds(glyph("waiting", session.id)).is_none());
        click(&shell, visual, item.into());
        wait(visual, |cx| {
            !shell.read(cx).session_unread(node, session.id)
        });
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.session_scope.active),
            Key::Session(node, session.id)
        );
        assert!(visual.debug_bounds("header-activity").is_none());
        assert!(
            visual
                .debug_bounds(selector(format!("live-session-{}", session.id)))
                .is_some()
        );
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

fn show_preview(visual: &mut VisualTestContext, item: &'static str) {
    hover(visual, item);
    visual.executor().advance_clock(Duration::from_millis(650));
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
    let preview = visual
        .debug_bounds(selector(format!("{item}-preview")))
        .unwrap();
    let title = visual
        .debug_bounds(selector(format!("{item}-preview-title")))
        .unwrap();
    let project = visual
        .debug_bounds(selector(format!("{item}-preview-project")))
        .unwrap();
    let trigger = visual.debug_bounds(item).unwrap();
    let viewport = visual.update(|window, _| window.viewport_size());
    assert_eq!(preview.size.width, px(280.));
    assert!(preview.left() >= px(0.) && preview.right() <= viewport.width);
    assert!(
        preview.top() >= trigger.bottom(),
        "preview must not cover its trigger"
    );
    assert!(title.size.height <= px(40.));
    assert!(project.top() > title.bottom());
    assert!(project.size.height <= px(20.));
    assert!(visual.debug_bounds("tooltip-popup").is_none());
}

fn session(template: &Session, status: Status, waiting: Option<Waiting>) -> Session {
    let mut session = template.clone();
    session.id = SessionId::new();
    session.activity.title = "Inspect the active workspace and verify every changed file before proceeding with the next operation. ".repeat(6);
    session.activity.attention.unread = true;
    session.activity.waiting = waiting;
    session.activity.run = Some(Run {
        worktree: session.worktree,
        session: session.id,
        turn: TurnId::new(),
        kind: RunKind::Task,
        sequence: 1,
        revision: 1,
        status,
        error: None,
        started_ms: None,
        finished_ms: None,
        origin: None,
    });
    session
}

#[gpui::test]
fn groups_spaced_indicators(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1440.), px(900.)));
    open(&shell, visual, &fixture, 0, &fixture.sessions[0]);
    let node = fixture.nodes[0].id();
    let template = &fixture.sessions[0];
    let running: Vec<_> = (0..6)
        .map(|_| session(template, Status::Running, None))
        .collect();
    let approval = session(template, Status::Running, Some(Waiting::Approval));
    let input = session(template, Status::Running, Some(Waiting::Input));
    let failed = session(template, Status::Failed, None);
    let done = session(template, Status::Completed, None);
    let mut read = session(template, Status::Completed, None);
    read.activity.attention.unread = false;
    let mut child = session(template, Status::Running, None);
    child.delegation = Some(Box::new(Delegation {
        session: template.id,
        turn: TurnId::new(),
        entry: "child".into(),
        index: 0,
        role: None,
    }));
    let mut archived = session(template, Status::Running, None);
    archived.archived = true;
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let snapshot = shell.live.as_mut().unwrap().view.snapshot.as_mut().unwrap();
            snapshot.cursor += 1;
            snapshot.sessions.extend(running.clone());
            snapshot.sessions.extend([
                approval.clone(),
                input.clone(),
                failed.clone(),
                done.clone(),
                read.clone(),
                child.clone(),
                archived.clone(),
            ]);
            cx.notify();
        })
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    for (state, session) in [
        ("running", &running[0]),
        ("waiting", &approval),
        ("failed", &failed),
        ("completed", &done),
    ] {
        let bounds = visual.debug_bounds(glyph(state, session.id)).unwrap();
        assert_eq!(bounds.size, size(px(20.), px(20.)));
    }
    for session in [&read, &child, &archived] {
        assert!(visual.debug_bounds(avatar(node, session.id)).is_none());
    }
    let first = visual.debug_bounds(avatar(node, running[0].id)).unwrap();
    let second = visual.debug_bounds(avatar(node, running[1].id)).unwrap();
    assert_eq!(second.left() - first.right(), px(4.));
    let first_glyph = visual
        .debug_bounds(glyph("running", running[0].id))
        .unwrap();
    let second_glyph = visual
        .debug_bounds(glyph("running", running[1].id))
        .unwrap();
    assert_eq!(second_glyph.left() - first_glyph.right(), px(12.));
    assert!(visual.debug_bounds(avatar(node, running[2].id)).is_none());
    for (left, right) in [("waiting", "running"), ("running", "completed")] {
        let left = visual
            .debug_bounds(selector(format!("header-activity_{left}")))
            .unwrap();
        let right = visual
            .debug_bounds(selector(format!("header-activity_{right}")))
            .unwrap();
        assert_eq!(right.left() - left.right(), px(12.));
    }
    let waiting = visual.debug_bounds("header-activity_waiting").unwrap();
    for session in [&approval, &input, &failed] {
        let item = visual.debug_bounds(avatar(node, session.id)).unwrap();
        assert!(
            waiting.contains(&item.center()),
            "waiting group {waiting:?} must contain {item:?} for {:?}",
            lane(session)
        );
    }
    // The overflow popover contains every current entry, including hidden shortcuts.
    let overflow = visual.debug_bounds("header-activity_running-more").unwrap();
    hover(visual, "header-activity_running-more");
    assert_eq!(
        visual.debug_bounds("header-activity_running-more").unwrap(),
        overflow
    );
    click(&shell, visual, "header-activity_running-more".into());
    visual.update(|window, cx| window.draw(cx).clear(cx));
    for session in &running {
        assert!(
            visual
                .debug_bounds(selector(format!(
                    "header-activity-row-{node:?}-{}",
                    session.id
                )))
                .is_some()
        );
    }
    let row = selector(format!("header-activity-row-{node:?}-{}", running[0].id));
    let list = visual.debug_bounds("header-activity-list").unwrap();
    let bounds = visual.debug_bounds(row).unwrap();
    assert!(
        bounds.right() <= list.right(),
        "hover wrapper must preserve row width"
    );
    show_preview(visual, row);
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("header-activity-list").is_none());
    // A narrow main pane with a preview still retains the toolbar and distinct groups.
    click(&shell, visual, "toggle-details".into());
    visual.simulate_window_resize(handle, size(px(1050.), px(900.)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let header = visual.debug_bounds("shell-module-header").unwrap();
    let strip = visual.debug_bounds("header-activity").unwrap();
    let tools = visual.debug_bounds("header-actions").unwrap();
    let toggle = visual.debug_bounds("toggle-details").unwrap();
    assert!(
        strip.left() >= header.left(),
        "strip {strip:?}, header {header:?}"
    );
    assert!(
        strip.right() <= tools.left(),
        "strip {strip:?}, tools {tools:?}"
    );
    assert!(
        toggle.right() <= header.right(),
        "toggle {toggle:?}, header {header:?}"
    );
    assert!(
        visual
            .debug_bounds("header-activity_waiting-more")
            .is_some()
    );
    assert!(visual.debug_bounds(avatar(node, approval.id)).is_some());
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let snapshot = shell.live.as_mut().unwrap().view.snapshot.as_mut().unwrap();
            snapshot
                .sessions
                .retain(|session| session.id == template.id || session.id == done.id);
            cx.notify();
        })
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("header-activity_running").is_none());
    assert!(visual.debug_bounds("header-activity_waiting").is_none());
    assert_eq!(
        visual.debug_bounds("header-activity").unwrap().size.width,
        px(28.)
    );
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
