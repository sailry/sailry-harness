use super::*;
use crate::conversation::live::tests::{
    fixture::{Fixture, init, open, tap},
    wait,
};
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt as _;
use sailry_protocol::{Permission, WorkMode, conversation::ApprovalSource};
use serde_json::json;

mod lifecycle;

#[gpui::test]
fn commands_preserve_shared_controls_and_markers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("/plan");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx).current_mode() == WorkMode::Plan && !view.read(cx).busy()
        });
        assert_eq!(view.read_with(visual, |view, cx| view.draft(cx)), "/plan ");
        assert_eq!(fixture.task_requests(), 0);
        visual.simulate_input("Inspect only 中文 🙂");
        view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            assert_eq!(input.value(), "/plan Inspect only 中文 🙂");
            assert_eq!(input.tokens().len(), 1);
            assert_eq!(input.tokens()[0].token().text(), "/plan");
        });
        visual.simulate_keystrokes("escape");
        select(&view, visual, WorkMode::Code);
        assert_eq!(
            view.read_with(visual, |view, cx| view.draft(cx)),
            "/code Inspect only 中文 🙂"
        );
        view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            assert_eq!(input.tokens().len(), 1);
            assert_eq!(input.tokens()[0].token().text(), "/code");
        });
        tap(visual, "live-chat-input");
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("```text\n/plan\n```\n/tmp/file");
        wait(visual, |_| true);
        assert_eq!(
            view.read_with(visual, |view, _| view.current_mode()),
            WorkMode::Code
        );
        assert_eq!(fixture.task_requests(), 0);
        visual.simulate_keystrokes("cmd-a");
        visual.simulate_input("/plan Plan this task");
        wait(visual, |cx| {
            view.read(cx).current_mode() == WorkMode::Plan && !view.read(cx).busy()
        });
        visual.simulate_keystrokes("escape");
        tap(visual, "live-chat-send");
        crate::conversation::live::tests::continue_plan(&view, visual);
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
                        .any(|run| run.status == Status::Completed)
                })
        });
        let requests = fixture.server.requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request["messages"].to_string().contains("Plan this task"))
        );
        assert!(!requests.iter().any(|request| {
            request["messages"]
                .to_string()
                .contains("/plan Plan this task")
        }));
        drop(requests);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

fn select(view: &Entity<View>, cx: &mut VisualTestContext, mode: WorkMode) {
    tap(cx, "live-chat-mode");
    tap(cx, &format!("{}-option", mode::label(mode)));
    wait(cx, |cx| {
        let view = view.read(cx);
        !view.pending && view.config.as_ref().unwrap().mode == mode
    });
}

fn session(view: &Entity<View>, cx: &mut VisualTestContext) -> Session {
    view.read_with(cx, |view, _| view.session.clone().unwrap())
}

fn menu_insets(
    cx: &mut VisualTestContext,
    first: &'static str,
    last: &'static str,
    first_content: &'static str,
    last_content: &'static str,
    last_index: usize,
) {
    let panel = cx.debug_bounds("popup-menu").unwrap();
    let quad = crate::ui::overlays::tests::panel(cx, "popup-menu");
    let (scale, radius, inset, padding, first_row, last_row) = cx.update(|window, cx| {
        (
            window.scale_factor(),
            cx.theme().radius,
            window.rem_size() * 0.25,
            window.rem_size() * 0.5,
            window.within("popup-menu").find(0usize).bounds(),
            window.within("popup-menu").find(last_index).bounds(),
        )
    });
    assert_eq!(quad.corner_radii, Corners::all(radius).scale(scale));
    let first_content = cx.debug_bounds(first_content).unwrap();
    let last_content = cx.debug_bounds(last_content).unwrap();
    let first = cx.debug_bounds(first).unwrap();
    let last = cx.debug_bounds(last).unwrap();
    assert_eq!(first, first_row);
    assert_eq!(last, last_row);
    for edge in [
        first_row.left() - panel.left(),
        first_row.top() - panel.top(),
        panel.right() - first_row.right(),
        last_row.left() - panel.left(),
        panel.right() - last_row.right(),
        panel.bottom() - last_row.bottom(),
    ] {
        assert!(
            (f32::from(edge) - f32::from(inset)).abs() < 0.01,
            "menu inset {edge:?}, expected {inset:?}; panel {panel:?}, first {first:?}, last {last:?}"
        );
    }
    for (content, row) in [(first_content, first_row), (last_content, last_row)] {
        assert_eq!(content.left() - row.left(), px(8.));
        assert_eq!(row.right() - content.right(), px(8.));
        assert!((content.top() - row.top() - padding).abs() < px(0.01));
        assert!((row.bottom() - content.bottom() - padding).abs() < px(0.01));
        assert!((row.center().y - content.center().y).abs() < px(0.01));
    }
}

#[gpui::test]
fn preserves_queue_and_focus(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| {
        gpui_kit::component::Theme::global_mut(cx).radius_lg = px(12.);
    });
    for remote in [false, true] {
        let fixture = Fixture::with_server(remote, |runtime| {
            runtime.block_on(crate::agent_fixture::Server::turn_tools(vec![(
                crate::agent_fixture::plugin_tool("files", "write_file"),
                json!({"path": "mode.txt", "text": "Coding output 中文 🙂", "expected_revision": null}),
            )]))
        });
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Draft 中文 🙂");
        tap(visual, "live-chat-mode");
        menu_insets(
            visual,
            "composer_mode_code-option",
            "composer_mode_plan-option",
            "composer_mode_code-content",
            "composer_mode_plan-content",
            1,
        );
        visual.simulate_keystrokes("escape");
        visual.simulate_input(" retained");
        tap(visual, "live-chat-mode");
        visual.simulate_keystrokes("down down enter");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().mode == WorkMode::Plan && !view.read(cx).pending
        });
        visual.simulate_input(" after selection");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Draft 中文 🙂 retained after selection"
        );
        tap(visual, "live-chat-permission");
        menu_insets(
            visual,
            "composer_permission_ask-option",
            "composer_permission_full-option",
            "composer_permission_ask-content",
            "composer_permission_full-content",
            2,
        );
        tap(visual, "composer_permission_full-option");
        wait(visual, |cx| {
            view.read(cx).config.as_ref().unwrap().permission == Permission::Full
                && !view.read(cx).pending
        });
        let current = session(&view, visual);
        let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
            session: current.id,
            expected_revision: current.revision,
            message: "Plan the change".into(),
        }) else {
            panic!("turn expected")
        };
        assert_eq!(turn.config.mode, WorkMode::Plan);
        select(&view, visual, WorkMode::Code);
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .queue
                .items
                .len()
                == 1
        });
        tap(visual, &format!("queue-send-{}", turn.id));
        crate::conversation::live::tests::continue_plan(&view, visual);
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .iter()
                .any(|run| {
                    run.turn == turn.id && matches!(run.status, Status::Completed | Status::Failed)
                })
        });
        assert!(!fixture.directory.path().join("project/mode.txt").exists());
        view.read_with(visual, |view, cx| {
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .approvals
                    .is_empty()
            );
            assert_eq!(
                view.input.read(cx).value(),
                "Draft 中文 🙂 retained after selection"
            );
        });
        visual.simulate_keystrokes("escape");
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .len()
                == 2
                && view
                    .read(cx)
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .all(|run| matches!(run.status, Status::Completed | Status::Failed))
                && !view.read(cx).pending
        });
        view.read_with(visual, |view, cx| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            assert_eq!(page.approvals.len(), 1, "runs: {:?}", page.runs);
            assert_eq!(page.approvals[0].source, ApprovalSource::Full);
            assert!(view.input.read(cx).value().is_empty());
        });
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project/mode.txt")).unwrap(),
            "Coding output 中文 🙂"
        );
        let current = session(&view, visual);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, fixture.binding.clone(), current.clone());
        wait(visual, |cx| view.read(cx).connected());
        assert_eq!(session(&view, visual).config, current.config);
        assert!(
            fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request["messages"]
                    .to_string()
                    .contains("This is a planning turn"))
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn merges_controller_changes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-mode");
        let mut config = fixture.session.config.clone();
        config.effort = Effort::Low;
        config.permission = Permission::Full;
        fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 2
        });
        tap(visual, "composer_mode_plan-option");
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 3 && !view.read(cx).pending
        });
        let current = session(&view, visual);
        assert_eq!(current.config.mode, WorkMode::Plan);
        assert_eq!(current.config.effort, Effort::Low);
        assert_eq!(current.config.permission, Permission::Full);
        select(&view, visual, WorkMode::Plan);
        assert_eq!(session(&view, visual).revision, 3);
        tap(visual, "live-chat-mode");
        visual.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.node.connected = false;
                cx.notify();
            })
        });
        tap(visual, "composer_mode_code-option");
        assert_eq!(session(&view, visual).config.mode, WorkMode::Plan);
        assert_eq!(session(&view, visual).revision, 3);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
