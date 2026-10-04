use super::workspace::click;
use super::*;
use crate::conversation::{
    fixture::InteractionScene,
    interaction::{Draft, Field, Kind, Response, State},
    turn::{Block, Status, Target},
};

fn show(shell: &Entity<Shell>, cx: &mut VisualTestContext, scene: InteractionScene) -> Target {
    let target = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_session((0, 2), window, cx);
            let turn = shell.conversations[&(0, 2)].turns.len();
            shell.show_interaction_preview((0, 2), scene, window, cx);
            Target {
                session: (0, 2),
                turn,
                request: 0,
                generation: 0,
            }
        })
    });
    draw(cx);
    target
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

fn state(shell: &Entity<Shell>, cx: &mut VisualTestContext, target: Target) -> State {
    cx.update(|_, cx| {
        let Block::Interaction(request) =
            &shell.read(cx).conversations[&target.session].turns[target.turn].blocks[0]
        else {
            panic!("missing interaction")
        };
        request.state.clone()
    })
}

fn input(cx: &mut VisualTestContext, value: &str) {
    click(cx, "question-input");
    cx.simulate_keystrokes("secondary-a");
    cx.simulate_input(value);
    draw(cx);
}

#[gpui::test]
fn single_choice_drafts(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx, InteractionScene::Single);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| input.set_value("Next draft", window, cx))
    });
    click(&mut cx, "interaction-submit");
    assert_eq!(state(&shell, &mut cx, target), State::Pending);
    click(&mut cx, "question-choice-0");
    click(&mut cx, "question-choice-1");
    input(&mut cx, "More context");
    cx.simulate_keystrokes("enter");
    draw(&mut cx);
    assert_eq!(state(&shell, &mut cx, target), State::Pending);
    click(&mut cx, "interaction-submit");
    assert_eq!(
        state(&shell, &mut cx, target),
        State::Answered(vec![tr("interaction_option_tests"), "More context".into()])
    );
    assert!(cx.debug_bounds("pending-interaction").is_none());
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].status, Status::Completed);
        assert_eq!(thread.input.read(cx).value(), "Next draft");
        assert_eq!(
            thread.turns[0].copy_text(),
            tr("interaction_preview_result").as_ref()
        );
        assert!(thread.interaction_drafts.is_empty());
        assert!(thread.preview_task.is_none());
    });
}

#[gpui::test]
fn multiple_and_freeform_choices(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx, InteractionScene::Multiple);
    click(&mut cx, "question-choice-1");
    click(&mut cx, "question-choice-0");
    click(&mut cx, "question-choice-1");
    input(&mut cx, " Extra checks ");
    click(&mut cx, "interaction-submit");
    assert_eq!(
        state(&shell, &mut cx, target),
        State::Answered(vec![tr("interaction_option_files"), "Extra checks".into()])
    );
    let target = show(&shell, &mut cx, InteractionScene::Single);
    input(&mut cx, "Only freeform");
    click(&mut cx, "interaction-submit");
    assert_eq!(
        state(&shell, &mut cx, target),
        State::Answered(vec!["Only freeform".into()])
    );
}

#[gpui::test]
fn text_input_limits(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx, InteractionScene::Text);
    cx.update(|_, cx| {
        shell.update(cx, |shell, _| {
            if let Block::Interaction(request) =
                &mut shell.conversations.get_mut(&(0, 2)).unwrap().turns[0].blocks[0]
            {
                request.kind = Kind::Text {
                    multiline: true,
                    max_bytes: 10,
                };
            }
        })
    });
    input(&mut cx, "\u{00e9}".repeat(6).as_str());
    click(&mut cx, "interaction-submit");
    assert_eq!(state(&shell, &mut cx, target), State::Pending);
    input(&mut cx, "A\nB");
    click(&mut cx, "interaction-submit");
    assert_eq!(
        state(&shell, &mut cx, target),
        State::Answered(vec!["A\nB".into()])
    );
    cx.update(|window, cx| {
        shell.update(cx, |_, cx| {
            let kind = Kind::Text {
                multiline: false,
                max_bytes: 10,
            };
            let draft = Draft::new(&kind, window, cx);
            assert!(matches!(draft.field, Some(Field::Line(_))));
            assert!(draft.answer(&kind, cx).is_none());
        })
    });
}

#[gpui::test]
fn resolution_variants(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    for (selector, expected) in [
        ("interaction-secondary", State::ContinuePlanning),
        ("interaction-submit", State::StartCoding),
    ] {
        let target = show(&shell, &mut cx, InteractionScene::Plan);
        click(&mut cx, selector);
        assert_eq!(state(&shell, &mut cx, target), expected);
        cx.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.resolve_interaction(target, Response::Cancel, cx)
            })
        });
        assert_eq!(state(&shell, &mut cx, target), expected);
    }
    let target = show(&shell, &mut cx, InteractionScene::Secret);
    assert!(cx.debug_bounds("question-input").is_none());
    assert!(cx.debug_bounds("interaction-submit").is_none());
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.resolve_interaction(target, Response::Submit, cx)
        })
    });
    assert_eq!(state(&shell, &mut cx, target), State::Pending);
    click(&mut cx, "interaction-secondary");
    assert_eq!(state(&shell, &mut cx, target), State::Cancelled);
}

#[gpui::test]
fn target_lifecycle(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let local = show(&shell, &mut cx, InteractionScene::Single);
    click(&mut cx, "question-choice-0");
    input(&mut cx, "Local draft");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_host(1, window, cx);
            shell.select_session((1, 0), window, cx);
            shell.show_interaction_preview((1, 0), InteractionScene::Single, window, cx);
            shell.resolve_interaction(
                Target {
                    generation: 1,
                    ..local
                },
                Response::Submit,
                cx,
            );
            shell.select_answer(
                Target {
                    generation: 1,
                    ..local
                },
                1,
                true,
                cx,
            );
        })
    });
    assert_eq!(state(&shell, &mut cx, local), State::Pending);
    let remote = Target {
        session: (1, 0),
        turn: 1,
        ..local
    };
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.resolve_interaction(local, Response::Submit, cx)
        })
    });
    assert_eq!(
        state(&shell, &mut cx, local),
        State::Answered(vec![tr("interaction_option_files"), "Local draft".into()])
    );
    assert_eq!(state(&shell, &mut cx, remote), State::Pending);
    draw(&mut cx);
    click(&mut cx, "composer-send");
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.resolve_interaction(remote, Response::Cancel, cx);
            assert!(shell.conversations[&(1, 0)].interaction_drafts.is_empty());
        })
    });
    assert_eq!(state(&shell, &mut cx, remote), State::Cancelled);
}

#[gpui::test]
fn approval_and_trust(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx, InteractionScene::Single);
    cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let mut approval = crate::conversation::fixture::sample(3);
            shell.conversations.get_mut(&(0, 2)).unwrap().turns[0]
                .blocks
                .push(approval.blocks.remove(0));
            cx.notify();
        })
    });
    draw(&mut cx);
    assert!(cx.debug_bounds("pending-interaction").is_none());
    click(&mut cx, "approval-approve-0");
    assert_eq!(state(&shell, &mut cx, target), State::Pending);
    assert!(cx.debug_bounds("pending-interaction").is_some());
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.project_trust(0, None, window, cx)));
    crate::prompts::tests::answer(&mut cx, "project_revoke");
    assert_eq!(state(&shell, &mut cx, target), State::Cancelled);
    assert!(cx.debug_bounds("pending-interaction").is_none());
}

#[gpui::test]
fn keyboard_focus_return(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    let target = show(&shell, &mut cx, InteractionScene::Multiple);
    click(&mut cx, "question-input");
    // Checkbox activation completes on key-up; GPUI's text helper emits key-down only.
    for key in ["shift-tab", "space"] {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        draw(&mut cx);
    }
    draw(&mut cx);
    cx.update(|_, cx| {
        assert_eq!(
            shell.read(cx).conversations[&target.session].interaction_drafts[&target]
                .selected
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [1]
        );
    });
    click(&mut cx, "interaction-submit");
    assert_eq!(
        state(&shell, &mut cx, target),
        State::Answered(vec![tr("interaction_option_tests")])
    );
    cx.simulate_input("Next prompt");
    cx.update(|_, cx| {
        assert_eq!(
            shell.read(cx).conversations[&target.session]
                .input
                .read(cx)
                .value(),
            "Next prompt"
        );
    });
}

#[gpui::test]
fn long_menu_scrolling(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.select_session((0, 2), window, cx)));
    click(&mut cx, "header-more");
    cx.simulate_keystrokes("down down enter");
    draw(&mut cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            draw(&mut cx);
            let pending = cx.debug_bounds("pending-interaction").unwrap();
            let surface = cx.debug_bounds("composer-surface").unwrap();
            assert_eq!(pending.left(), surface.left());
            assert_eq!(pending.right(), surface.right());
            assert!(pending.bottom() < surface.top());
            assert!(pending.size.height <= px(224.));
            for selector in [
                "question-choice-0",
                "question-choice-1",
                "question-input",
                "interaction-secondary",
                "interaction-submit",
            ] {
                let bounds = cx.debug_bounds(selector).unwrap();
                assert!(bounds.left() >= pending.left());
                assert!(bounds.right() <= pending.right());
                assert!(bounds.bottom() <= pending.bottom());
            }
        }
    }
    let handle = cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            if let Block::Interaction(request) =
                &mut shell.conversations.get_mut(&(0, 2)).unwrap().turns[0].blocks[0]
            {
                request.prompt = "A long question that needs careful review. "
                    .repeat(30)
                    .into();
            }
            cx.notify();
        });
        window.window_handle()
    });
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    draw(&mut cx);
    let pending = cx.debug_bounds("pending-interaction").unwrap();
    let submit = cx.debug_bounds("interaction-submit").unwrap();
    assert!(pending.size.height <= px(224.));
    assert!(submit.bottom() <= pending.bottom());
    cx.simulate_event(ScrollWheelEvent {
        position: pending.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-4000.))),
        ..Default::default()
    });
    draw(&mut cx);
    let field = cx.debug_bounds("question-input").unwrap();
    assert!(field.top() >= pending.top());
    assert!(field.bottom() < submit.top());
    input(&mut cx, "Scrolled answer");
    click(&mut cx, "interaction-submit");
    assert!(cx.debug_bounds("pending-interaction").is_none());
}
