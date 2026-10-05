use super::*;

#[gpui::test]
fn releases_initialized_draft(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = crate::conversation::live::ChatFixture::with_tools(false, Vec::new());
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::backend::Services {
            runtime: fixture.runtime.clone(),
            local: fixture.node.local(),
            link: fixture.node.link(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        shell.update(cx, |shell, cx| shell.new_live_conversation(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    let draft = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().downgrade());
    visual.update(|window, _| window.remove_window());
    drop(shell);
    visual.run_until_parked();
    cx.update(|_| {});
    draft.assert_released();
    fixture.close();
}

fn frame(cx: &mut VisualTestContext, elapsed: u64) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(elapsed));
    cx.run_until_parked();
    cx.update(|window, cx| {
        _ = window.draw(cx);
    });
}

#[gpui::test]
fn empty_preview(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session_scope.open.clear();
            shell.select_session((0, 2), window, cx);
        });
    });
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("session-tabs").is_none());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.conversations[&(0, 2)].input.update(cx, |input, cx| {
                input.set_value("Preview conversation", window, cx)
            });
        });
    });
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("session-tabs").is_none());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.send_preview((0, 2), window, cx));
    });
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("session-tabs").is_none());
}

#[gpui::test]
fn welcome_and_composer(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session = 2;
            shell.navigate(Page::Conversation, window, cx);
        });
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (width, height) in [(760., 560.), (1280., 820.), (1920., 1080.)] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(height)));
            frame(&mut cx, 400);
            let page = cx.debug_bounds("conversation-page").unwrap();
            let composer = cx.debug_bounds("conversation-composer").unwrap();
            let context = cx.debug_bounds("composer-context-bar").unwrap();
            let surface = cx.debug_bounds("composer-surface").unwrap();
            let input = cx.debug_bounds("composer-input").unwrap();
            let toolbar = cx.debug_bounds("composer-toolbar").unwrap();
            assert!(cx.debug_bounds("composer-stats").is_none());
            let send = cx.debug_bounds("composer-send").unwrap();
            assert!(cx.debug_bounds("conversation-welcome").is_some());
            assert!(composer.size.width <= px(crate::conversation::CONTENT_WIDTH));
            assert!((composer.center().x - page.center().x).abs() <= px(1.));
            assert!(composer.bottom() <= page.bottom());
            assert!(cx.debug_bounds("composer-activity").is_none());
            assert!(input.bottom() <= toolbar.top());
            assert!(toolbar.bottom() < context.top());
            assert!((surface.bottom() - context.top()).abs() <= px(1.));
            assert_eq!(surface.left() + px(12.), context.left());
            assert_eq!(surface.right() - px(12.), context.right());
            assert_eq!(surface.center().x, context.center().x);
            let progress = cx.debug_bounds("composer-compression").unwrap();
            assert!(progress.right() < send.left());
            assert!((progress.center().y - send.center().y).abs() <= px(1.));
            for selector in ["composer-host", "composer-branch"] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(control.left() >= composer.left());
                assert!(control.right() <= composer.right());
                assert!(control.top() >= context.top());
                assert!(control.bottom() <= context.bottom());
            }
            assert!(send.right() <= composer.right());
        }
    }
    let suggestion = cx.debug_bounds("welcome_explore").unwrap();
    cx.simulate_click(suggestion.center(), Modifiers::default());
    frame(&mut cx, 0);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        tr("welcome_explore_prompt")
    );
    let wordmark = cx.debug_bounds("welcome-dots").unwrap();
    let welcome = cx.debug_bounds("conversation-welcome").unwrap();
    for position in [
        point(wordmark.left() + px(1.), wordmark.center().y),
        wordmark.center(),
        point(wordmark.right() - px(1.), wordmark.center().y),
        point(welcome.right() - px(1.), wordmark.center().y),
        point(px(1.), px(1.)),
    ] {
        cx.simulate_mouse_move(position, None, Modifiers::default());
        frame(&mut cx, 0);
        cx.update(|window, cx| {
            let input = shell.read(cx).conversations[&(0, 2)].input.read(cx);
            assert_eq!(input.value(), tr("welcome_explore_prompt"));
            assert!(input.focus_handle(cx).is_focused(window));
        });
    }
    cx.simulate_keystrokes("shift-enter");
    frame(&mut cx, 0);
    assert!(cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.is_empty()));
    cx.simulate_keystrokes("enter");
    frame(&mut cx, 0);
    cx.update(|_, cx| {
        let conversation = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(conversation.turns.len(), 1);
        assert!(conversation.input.read(cx).value().is_empty());
    });
    let context = cx.debug_bounds("composer-context-bar").unwrap();
    let stats = cx.debug_bounds("composer-stats").unwrap();
    assert!(stats.top() >= context.top() && stats.bottom() <= context.bottom());
    for selector in [
        "composer_tokens",
        "composer_speed",
        "composer_cost",
        "composer_cache",
        "composer_turns",
    ] {
        let metric = cx.debug_bounds(selector).unwrap();
        assert!(metric.left() >= context.left() && metric.right() <= context.right());
        assert!(metric.top() >= stats.top() && metric.bottom() <= stats.bottom());
    }
    assert!(cx.debug_bounds("conversation-welcome").is_none());
    assert!(cx.debug_bounds("composer-activity").is_some());
}

#[gpui::test]
fn activity_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    frame(&mut cx, 400);
    let activity = cx.debug_bounds("composer-activity").unwrap();
    let surface = cx.debug_bounds("composer-surface").unwrap();
    let todo = cx.debug_bounds("composer-todo").unwrap();
    let changes = cx.debug_bounds("composer-changes").unwrap();
    assert!(activity.bottom() < surface.top());
    assert!(todo.right() < changes.left());
    assert!((todo.top() - changes.top()).abs() <= px(1.));
    cx.simulate_click(todo.center(), Modifiers::default());
    frame(&mut cx, 200);
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    let popup = cx.debug_bounds("composer-todo-content").unwrap();
    assert!(popup.bottom() < todo.top());
    for selector in [
        "composer-todo-task-0",
        "composer-todo-task-1",
        "composer-todo-task-4",
    ] {
        assert!(cx.debug_bounds(selector).is_some());
    }
    let collapse = cx.debug_bounds("composer-todo-collapse").unwrap();
    cx.simulate_click(collapse.center(), Modifiers::default());
    frame(&mut cx, 200);
    assert!(cx.debug_bounds("composer-todo-content").is_none());
    cx.simulate_click(todo.center(), Modifiers::default());
    frame(&mut cx, 200);
    cx.simulate_keystrokes("escape");
    frame(&mut cx, 200);
    assert!(cx.debug_bounds("composer-todo-content").is_none());
    cx.simulate_click(changes.center(), Modifiers::default());
    frame(&mut cx, 400);
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).side_resource.as_ref().unwrap().page()),
        Page::Git
    );
    assert!(cx.debug_bounds("resource-side-panel").is_some());
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Conversation, window, cx)
        })
    });
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("composer-activity").is_some());
}

#[gpui::test]
fn target_drafts(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session = 2;
            shell.navigate(Page::Conversation, window, cx);
            shell.conversations[&(0, 2)]
                .input
                .update(cx, |input, cx| input.set_value("Local draft", window, cx));
            shell
                .conversations
                .get_mut(&(0, 2))
                .unwrap()
                .options
                .choices = [1, 1, 0, 2];
            shell
                .workspace
                .sessions
                .get_mut(&(0, 2))
                .unwrap()
                .owner
                .worktree = 2;
            shell.select_composer_host(1, window, cx);
            assert_eq!(shell.session, 2);
            assert!(
                shell.conversations[&(1, 2)]
                    .input
                    .read(cx)
                    .value()
                    .is_empty()
            );
            assert_eq!(shell.conversations[&(1, 2)].options.choices, [0; 4]);
            shell.select_composer_host(0, window, cx);
            assert_eq!(
                shell.conversations[&(0, 2)].input.read(cx).value(),
                "Local draft"
            );
            assert_eq!(shell.conversations[&(0, 2)].options.choices, [1, 1, 0, 2]);
        })
    });
    frame(&mut cx, 0);
    assert!(cx.debug_bounds("conversation-welcome").is_some());
}

#[gpui::test]
fn target_controls(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session = 2;
            shell.navigate(Page::Conversation, window, cx);
        })
    });
    frame(&mut cx, 400);
    for (selector, field, choice) in [
        ("composer-mode", 0, 1),
        ("composer-branch", 1, 1),
        ("composer-permission", 3, 2),
    ] {
        let control = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(control.center(), Modifiers::default());
        frame(&mut cx, 200);
        let items: &[&str] = match field {
            0 => &["composer_mode_code-option", "composer_mode_plan-option"],
            3 => &[
                "composer_permission_ask-option",
                "composer_permission_project-option",
                "composer_permission_full-option",
            ],
            _ => &[],
        };
        for item in items {
            let content = cx.debug_bounds(item).unwrap();
            assert!(content.size.height > px(0.));
            assert!(content.bottom() < control.top());
        }
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        frame(&mut cx, 0);
        for _ in 0..=choice {
            cx.simulate_keystrokes("down");
            frame(&mut cx, 0);
        }
        cx.simulate_keystrokes("enter");
        frame(&mut cx, 0);
        assert_eq!(
            cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].options.choices[field]),
            choice,
            "{selector}"
        );
    }
    let host = cx.debug_bounds("composer-host").unwrap();
    cx.simulate_click(host.center(), Modifiers::default());
    frame(&mut cx, 200);
    cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
    frame(&mut cx, 0);
    for key in ["down", "down", "enter"] {
        cx.simulate_keystrokes(key);
        frame(&mut cx, 0);
    }
    frame(&mut cx, 0);
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        assert_eq!(shell.host, 1);
        assert_eq!(shell.session, 2);
        assert_eq!(shell.conversations[&(1, 2)].options.choices, [0; 4]);
        assert_eq!(shell.conversations[&(0, 2)].options.choices[1], 1);
        assert_eq!(shell.conversations[&(0, 2)].options.choices[3], 2);
    });
    assert!(cx.debug_bounds("conversation-welcome").is_some());
    assert!(cx.debug_bounds("composer-activity").is_none());
}

#[gpui::test]
fn model_picker(cx: &mut TestAppContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(820.)));
            frame(&mut cx, 400);
            let trigger = cx.debug_bounds("composer-model").unwrap();
            cx.simulate_mouse_move(trigger.center(), None, Modifiers::default());
            frame(&mut cx, 200);
            cx.simulate_click(trigger.center(), Modifiers::default());
            frame(&mut cx, 200);
            let picker = cx.debug_bounds("composer-model-picker").unwrap();
            assert!(picker.left() >= px(0.));
            assert!(picker.right() <= px(width));
            assert!(picker.top() >= px(0.));
            assert!(picker.bottom() <= px(820.));
            assert!(picker.bottom() < trigger.top());
            let model = cx
                .debug_bounds("composer-model-option-0-preview-text-1")
                .unwrap();
            assert!(model.size.height <= px(36.));
            assert_eq!(
                cx.update(|_, cx| shell.read(cx).selected_model(cx).unwrap().1.id.clone()),
                "preview-text-1"
            );
            cx.simulate_keystrokes("escape");
            frame(&mut cx, 200);
            assert!(cx.debug_bounds("composer-model-picker").is_none());
        }
    }
    let trigger = cx.debug_bounds("composer-model").unwrap();
    cx.simulate_click(trigger.center(), Modifiers::default());
    frame(&mut cx, 200);
    assert!(cx.debug_bounds("composer-model-family-2").is_none());
    assert!(cx.debug_bounds("composer-model-family-4").is_none());
    assert!(cx.debug_bounds("composer-model-empty").is_none());
    cx.simulate_keystrokes("escape");
    frame(&mut cx, 200);
    let trigger = cx.debug_bounds("composer-model").unwrap();
    cx.simulate_click(trigger.center(), Modifiers::default());
    frame(&mut cx, 200);
    assert!(
        cx.debug_bounds("composer-model-option-0-preview-text-1")
            .is_some()
    );
    assert!(cx.debug_bounds("composer-model-family-4").is_none());
    let all = cx.debug_bounds("composer-model-family-0").unwrap();
    cx.simulate_click(all.center(), Modifiers::default());
    frame(&mut cx, 200);
    let group = cx.debug_bounds("composer-model-group-0").unwrap();
    let expanded_height = cx
        .debug_bounds("composer-model-picker")
        .unwrap()
        .size
        .height;
    let expanded_content = cx
        .debug_bounds("composer-model-scroll")
        .unwrap()
        .size
        .height;
    cx.simulate_click(group.center(), Modifiers::default());
    frame(&mut cx, 400);
    // Kit keeps closed panels mounted; their content collapses inside the fixed picker.
    assert!(
        cx.debug_bounds("composer-model-scroll")
            .unwrap()
            .size
            .height
            < expanded_content
    );
    assert_eq!(
        cx.debug_bounds("composer-model-picker")
            .unwrap()
            .size
            .height,
        expanded_height
    );
    let group = cx.debug_bounds("composer-model-group-0").unwrap();
    cx.simulate_click(group.center(), Modifiers::default());
    frame(&mut cx, 400);
    let model = cx
        .debug_bounds("composer-model-option-0-preview-text-1")
        .unwrap();
    cx.simulate_click(model.center(), Modifiers::default());
    frame(&mut cx, 200);
    assert!(cx.debug_bounds("composer-model-picker").is_none());
    cx.update(|_, cx| {
        let shell = shell.read(cx);
        let options = &shell.conversations[&(0, 0)].options;
        assert_eq!(options.model.as_ref().unwrap().model, "preview-text-1");
        assert_eq!(options.effort, Some(sailry_protocol::Effort::Medium));
    });
}
