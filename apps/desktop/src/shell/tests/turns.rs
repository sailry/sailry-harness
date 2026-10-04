use super::*;
use crate::conversation::{
    fixture,
    turn::{Block, Phase, Status},
};
use std::time::Duration;

fn frame(cx: &mut VisualTestContext, milliseconds: u64) {
    cx.run_until_parked();
    cx.executor()
        .advance_clock(Duration::from_millis(milliseconds));
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        _ = window.draw(cx);
    });
}

fn start(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session = 2;
            shell.navigate(Page::Conversation, window, cx);
            shell.conversations[&(0, 2)].input.update(cx, |input, cx| {
                input.set_value("Preview prompt", window, cx);
                input.focus(window, cx);
            });
        })
    });
    frame(cx, 0);
    cx.simulate_keystrokes("enter");
    frame(cx, 0);
}

#[gpui::test]
fn streamed_steps_are_one_turn(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    start(&shell, &mut cx);
    for (elapsed, expected) in [
        (0, Status::Queued),
        (1000, Status::Running(Phase::Thinking)),
        (2000, Status::Running(Phase::Tools)),
        (3000, Status::Running(Phase::Answer)),
        (3000, Status::Completed),
    ] {
        frame(&mut cx, elapsed);
        cx.update(|_, cx| {
            let thread = &shell.read(cx).conversations[&(0, 2)];
            assert_eq!(thread.turns.len(), 1);
            assert_eq!(thread.scroller.read(cx).item_count(), 1);
            assert_eq!(thread.turns[0].status, expected);
            assert_eq!(thread.turns[0].prompt, "Preview prompt");
        });
        assert_eq!(cx.debug_bounds("turn-phase-0").is_some(), expected.active());
        assert_eq!(cx.debug_bounds("turn-orb-0").is_some(), expected.active());
        assert_eq!(
            cx.debug_bounds("turn-footer-0").is_some(),
            !expected.active()
        );
    }
    cx.update(|_, cx| {
        let turn = &shell.read(cx).conversations[&(0, 2)].turns[0];
        assert_eq!(turn.copy_text(), tr("preview_notice").as_ref());
        assert!(matches!(&turn.blocks[1], Block::Tools(calls) if calls.len() == 2));
    });
    let copy = cx.debug_bounds("turn-copy-0").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    assert_eq!(
        cx.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        tr("preview_notice").as_ref()
    );
}

#[gpui::test]
fn stops_partial_reply(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    start(&shell, &mut cx);
    frame(&mut cx, 6000);
    cx.simulate_input("Next draft");
    frame(&mut cx, 0);
    let partial = cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.input.read(cx).value(), "Next draft");
        thread.turns[0].copy_text()
    });
    assert!(!partial.is_empty());
    let stop = cx.debug_bounds("composer-send").unwrap();
    cx.simulate_click(stop.center(), Modifiers::default());
    frame(&mut cx, 10_000);
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns[0].status, Status::Cancelled);
        assert_eq!(thread.turns[0].copy_text(), partial);
        assert!(thread.preview_task.is_none());
        assert_eq!(thread.input.read(cx).value(), "Next draft");
    });
    cx.simulate_click(stop.center(), Modifiers::default());
    frame(&mut cx, 0);
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 2);
        assert_eq!(thread.turns[0].status, Status::Cancelled);
        assert_eq!(thread.turns[1].prompt, "Next draft");
    });
}

#[gpui::test]
fn preview_stream_ownership(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    start(&shell, &mut cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_composer_host(1, window, cx)));
    frame(&mut cx, 9000);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(
            shell.conversations[&(0, 2)].turns[0].status,
            Status::Completed
        );
        assert!(shell.conversations[&(1, 2)].turns.is_empty());
    });
    assert!(cx.debug_bounds("conversation-welcome").is_some());
}

#[gpui::test]
fn turn_geometry_and_groups(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280., 1920.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            for state in [0, 1, 2] {
                cx.update(|_, cx| {
                    shell.update(cx, |shell, cx| {
                        let thread = shell.conversations.get_mut(&(0, 0)).unwrap();
                        thread.turns = vec![fixture::sample(state)];
                        if state == 1 && width == 760. {
                            thread.turns[0]
                                .blocks
                                .push(Block::Error(tr("turn_preview_failure").repeat(4).into()));
                        }
                        thread.scroller.update(cx, |state, cx| state.reset(1, cx));
                        cx.set_reduce_motion(true);
                        cx.notify();
                    })
                });
                frame(&mut cx, 0);
                let row = cx.debug_bounds("turn-0").unwrap();
                let page = cx.debug_bounds("conversation-page").unwrap();
                let header = cx.debug_bounds("turn-header-0").unwrap();
                let composer = cx.debug_bounds("conversation-composer").unwrap();
                assert!(row.size.width <= px(crate::conversation::CONTENT_WIDTH));
                assert!((row.center().x - page.center().x).abs() <= px(1.));
                assert!(header.top() > row.top());
                assert!(header.right() <= row.right());
                if state == 1 && width == 760. {
                    let error = cx.debug_bounds("turn-error-0").unwrap();
                    assert!(error.size.height > px(20.), "narrow error text must wrap");
                    assert!(error.right() <= header.right());
                }
                let footer = cx
                    .debug_bounds(if state == 2 {
                        "turn-phase-0"
                    } else {
                        "turn-footer-0"
                    })
                    .unwrap();
                assert!(footer.top() > header.bottom());
                assert!(footer.bottom() < composer.top());
                if state != 2 {
                    let work = cx.debug_bounds("turn-work-0").unwrap();
                    cx.simulate_click(work.center(), Modifiers::default());
                    frame(&mut cx, 0);
                }
                let group = cx.debug_bounds("turn-group-0-1").unwrap();
                cx.simulate_click(group.center(), Modifiers::default());
                frame(&mut cx, 400);
                cx.update(|_, cx| {
                    assert_eq!(
                        shell.read(cx).conversations[&(0, 0)].turns[0]
                            .expanded
                            .get(&1),
                        Some(&(state != 2))
                    );
                });
            }
        }
    }
}

#[gpui::test]
fn tool_link_panels(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let work = cx.debug_bounds("turn-work-0").unwrap();
    cx.simulate_click(work.center(), Modifiers::default());
    frame(&mut cx, 0);
    let group = cx.debug_bounds("turn-group-0-1").unwrap();
    cx.simulate_click(group.center(), Modifiers::default());
    frame(&mut cx, 400);
    let link = cx.debug_bounds("turn-tool-link-0-1").unwrap();
    cx.simulate_click(
        point(link.left() + px(8.), link.center().y),
        Modifiers::default(),
    );
    frame(&mut cx, 400);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.file_state(true).tabs.selected, 1);
        assert!(shell.conversations[&(0, 0)].turns[0].expanded[&1]);
    });
    assert!(cx.debug_bounds("resource-side-panel").is_some());
}

#[gpui::test]
fn preserves_manual_disclosure(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    start(&shell, &mut cx);
    frame(&mut cx, 3000);
    assert!(cx.debug_bounds("turn-group-0-1").is_some());
    let check_spacing = |cx: &mut VisualTestContext| {
        let header = cx.debug_bounds("turn-header-0").unwrap();
        let trigger = cx.debug_bounds("turn-work-0").unwrap();
        let chevron = cx.debug_bounds("turn-work-0-chevron").unwrap();
        let first = cx.debug_bounds("turn-group-0-0").unwrap();
        let second = cx.debug_bounds("turn-group-0-1").unwrap();
        assert!(header.contains(&trigger.center()));
        assert_eq!(trigger.left(), header.left());
        assert_eq!(trigger.right(), header.right());
        assert!(trigger.contains(&chevron.center()));
        assert!(trigger.right() - chevron.right() <= px(8.));
        assert!(first.top() > header.bottom());
        assert_eq!(
            second.top() - first.bottom(),
            cx.update(|_, cx| cx.theme().spacing_tokens().lg)
        );
        assert!(cx.debug_bounds("turn-work-0-summary").is_none());
    };
    check_spacing(&mut cx);
    let work = cx.debug_bounds("turn-work-0").unwrap();
    cx.simulate_click(work.center(), Modifiers::default());
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("turn-group-0-1").is_none());
    frame(&mut cx, 3000);
    assert!(cx.debug_bounds("turn-group-0-1").is_none());
    assert!(cx.debug_bounds("turn-orb-0").is_some());
    assert!(cx.debug_bounds("turn-text-0-2").is_some());
    frame(&mut cx, 3000);
    assert!(cx.debug_bounds("turn-group-0-1").is_none());
    assert!(cx.debug_bounds("turn-orb-0").is_none());
    assert!(cx.debug_bounds("turn-copy-0").is_some());
    assert!(cx.debug_bounds("turn-text-0-2").is_some());
    let chevron = cx.debug_bounds("turn-work-0-chevron").unwrap();
    cx.simulate_click(chevron.center(), Modifiers::default());
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("turn-group-0-1").is_some());
    assert!(cx.debug_bounds("turn-copy-0").is_some());
    assert!(cx.debug_bounds("turn-text-0-2").is_some());
    check_spacing(&mut cx);
}

#[gpui::test]
fn preserves_reader_position(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let thread = shell.conversations.get_mut(&(0, 0)).unwrap();
            thread.turns = (0..30).map(|_| fixture::sample(0)).collect();
            thread.scroller.update(cx, |state, cx| {
                state.reset(30, cx);
                state.scroll_to_item(10, cx);
            });
            cx.notify();
        })
    });
    frame(&mut cx, 400);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let thread = shell.conversations.get_mut(&(0, 0)).unwrap();
            assert!(thread.scroller.read(cx).is_scrolled_up());
            fixture::advance(&mut thread.turns[29], Duration::from_secs(6));
            thread.scroller.update(cx, |state, cx| {
                state.remeasure_items(29..30, cx);
            });
            cx.notify();
        })
    });
    frame(&mut cx, 400);
    cx.update(|_, cx| {
        let scroller = shell.read(cx).conversations[&(0, 0)].scroller.clone();
        assert!(!scroller.read(cx).is_following_tail());
        scroller.update(cx, |state, cx| state.scroll_to_end(cx));
    });
    frame(&mut cx, 400);
    assert!(cx.update(|_, cx| {
        shell.read(cx).conversations[&(0, 0)]
            .scroller
            .read(cx)
            .is_following_tail()
    }));
}
