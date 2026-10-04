use super::workspace::click;
use super::*;
use crate::conversation::{
    approval::{State, Target},
    turn::{Block, Status},
};

fn show(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> Target {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            shell.show_approval_preview((0, 2), cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    Target {
        session: (0, 2),
        turn: 0,
        request: 0,
        generation: 0,
    }
}

fn states(shell: &Entity<Shell>, cx: &mut VisualTestContext, key: (usize, usize)) -> Vec<State> {
    cx.update(|_, cx| {
        shell.read(cx).conversations[&key]
            .turns
            .last()
            .unwrap()
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Approval(request) => Some(request.state),
                _ => None,
            })
            .collect()
    })
}

#[gpui::test]
fn independent_decisions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    show(&shell, &mut cx);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| input.set_value("Keep draft", window, cx))
    });
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Pending, State::Pending]
    );
    click(&mut cx, "approval-reject-1");
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Pending, State::Rejected]
    );
    assert!(cx.debug_bounds("approval-approve-0").is_some());
    assert!(cx.debug_bounds("approval-approve-1").is_none());
    click(&mut cx, "approval-approve-0");
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Approved, State::Rejected]
    );
    assert!(cx.debug_bounds("pending-approvals").is_none());
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].status, Status::Completed);
        assert_eq!(
            thread.turns[0].copy_text(),
            tr("approval_preview_result").as_ref()
        );
        assert!(thread.turns[0].finished_at.is_some());
        assert!(thread.preview_task.is_none());
        assert_eq!(thread.input.read(cx).value(), "Keep draft");
    });
}

#[gpui::test]
fn ignores_stale_decisions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.decide_preview(
                Target {
                    generation: 1,
                    ..target
                },
                State::Approved,
                cx,
            );
            shell.decide_preview(
                Target {
                    request: 99,
                    ..target
                },
                State::Approved,
                cx,
            );
            shell.decide_preview(target, State::Pending, cx);
        })
    });
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Pending, State::Pending]
    );
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.decide_preview(target, State::Rejected, cx);
            shell.decide_preview(target, State::Approved, cx);
        })
    });
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Rejected, State::Pending]
    );
    click(&mut cx, "composer-send");
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.decide_preview(
                Target {
                    request: 1,
                    ..target
                },
                State::Approved,
                cx,
            )
        })
    });
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Rejected, State::Cancelled]
    );
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns[0].status),
        Status::Cancelled
    );
    assert!(cx.debug_bounds("pending-approvals").is_none());
}

#[gpui::test]
fn binds_original_target(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_host(1, window, cx);
            shell.select_session((1, 0), window, cx);
            shell.show_approval_preview((1, 0), cx);
            shell.decide_preview(target, State::Approved, cx);
        })
    });
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Approved, State::Pending]
    );
    assert_eq!(
        states(&shell, &mut cx, (1, 0)),
        [State::Pending, State::Pending]
    );
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.show_approval_preview((1, 0), cx)));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(1, 0)].turns.len()),
        2
    );
}

#[gpui::test]
fn menu_and_geometry(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 2), window, cx)));
    click(&mut cx, "header-more");
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Pending, State::Pending]
    );
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            cx.update(|window, cx| {
                _ = window.draw(cx);
            });
            let approvals = cx.debug_bounds("pending-approvals").unwrap();
            let surface = cx.debug_bounds("composer-surface").unwrap();
            assert!(approvals.bottom() < surface.top());
            assert_eq!(approvals.left(), surface.left());
            assert_eq!(approvals.right(), surface.right());
            assert!(approvals.size.height <= px(192.));
            for selector in [
                "approval-approve-0",
                "approval-reject-0",
                "approval-approve-1",
                "approval-reject-1",
            ] {
                let button = cx.debug_bounds(selector).unwrap();
                assert!(button.right() <= approvals.right());
                assert!(button.bottom() <= approvals.bottom());
                assert!(button.size.width >= px(40.));
            }
        }
    }
}

#[gpui::test]
fn links_and_trust_revocation(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    show(&shell, &mut cx);
    let link = cx.debug_bounds("approval-link-1").unwrap();
    cx.simulate_click(
        point(link.left() + px(8.), link.center().y),
        Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("resource-side-panel").is_some());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert_eq!(shell.page, Page::Conversation);
            assert_eq!(shell.file_state(true).tabs.selected, 1);
            shell.close_resource_panel(cx);
            shell.project_trust(0, None, window, cx);
        })
    });
    crate::prompts::tests::answer(&mut cx, "project_revoke");
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Cancelled, State::Cancelled]
    );
    assert!(cx.debug_bounds("pending-approvals").is_none());
}

#[gpui::test]
fn long_request_bounds(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    show(&shell, &mut cx);
    let handle = cx.update(|window, cx| {
        shell.update(cx, |shell, _| {
            if let Block::Approval(request) =
                &mut shell.conversations.get_mut(&(0, 2)).unwrap().turns[0].blocks[0]
            {
                request.prompt = "A long approval request that needs careful review. "
                    .repeat(20)
                    .into();
            }
        });
        window.window_handle()
    });
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let approvals = cx.debug_bounds("pending-approvals").unwrap();
    let surface = cx.debug_bounds("composer-surface").unwrap();
    let context = cx.debug_bounds("composer-context-bar").unwrap();
    assert!(approvals.size.height <= px(192.));
    assert!(approvals.bottom() < surface.top());
    assert!(context.bottom() <= px(560.));
    let approve = cx.debug_bounds("approval-approve-0").unwrap();
    assert!(approve.right() <= approvals.right());
    assert!(approve.size.width >= px(48.));
    cx.simulate_event(ScrollWheelEvent {
        position: approvals.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-4000.))),
        ..Default::default()
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    let last = cx.debug_bounds("approval-reject-1").unwrap();
    assert!(last.top() >= approvals.top());
    assert!(
        last.bottom() <= approvals.bottom(),
        "last={last:?}, approvals={approvals:?}"
    );
    click(&mut cx, "approval-reject-1");
    assert_eq!(
        states(&shell, &mut cx, (0, 2)),
        [State::Pending, State::Rejected]
    );
}
