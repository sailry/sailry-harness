use super::*;
use crate::conversation::references::Kind;

fn frame(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    frame(cx);
}

fn mention(shell: &Entity<Shell>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| input.focus(window, cx));
    });
    cx.simulate_input("@");
    frame(cx);
}

fn draft(cx: &mut TestAppContext) -> (Entity<Shell>, VisualTestContext) {
    let (shell, mut cx) = setup(cx);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.session = 2;
            shell.navigate(Page::Conversation, window, cx);
            shell.conversations[&(0, 2)]
                .input
                .update(cx, |input, cx| input.focus(window, cx));
        })
    });
    frame(&mut cx);
    (shell, cx)
}

#[gpui::test]
fn keyboard_catalog_and_send(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    cx.simulate_input("Please @agents");
    frame(&mut cx);
    assert!(cx.debug_bounds("reference-picker").is_some());
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.is_empty()));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        "Please @"
    );
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    assert!(cx.debug_bounds("reference-picker").is_none());
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.input.read(cx).tokens().len(), 1);
        assert_eq!(
            thread.input.read(cx).value(),
            format!(
                "Please @{} ",
                thread.options.references[0].label.trim_start_matches('@')
            )
        );
        assert_eq!(thread.options.references.len(), 1);
        assert!(matches!(thread.options.references[0].kind, Kind::Agent(_)));
        assert!(thread.options.model.is_none());
    });
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 1);
        let reference = &thread.turns[0].options.as_ref().unwrap().references[0];
        assert_eq!(
            thread.turns[0].prompt,
            format!("Please @{} ", reference.label.trim_start_matches('@'))
        );
        assert_eq!(
            thread.turns[0].options.as_ref().unwrap().references.len(),
            1
        );
        assert!(thread.options.references.is_empty());
    });
}

#[gpui::test]
fn directory_pointer_actions(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    mention(&shell, &mut cx);
    click(&mut cx, "reference-row-0");
    click(&mut cx, "reference-row-1");
    assert!(cx.debug_bounds("reference-picker").is_some());
    cx.simulate_keystrokes("backspace");
    frame(&mut cx);
    click(&mut cx, "reference-row-1");
    click(&mut cx, "reference-row-0");
    cx.update(|_, cx| assert!(matches!(&shell.read(cx).conversations[&(0, 2)].options.references[0].kind, Kind::Directory(path) if path == "docs")));
    mention(&shell, &mut cx);
    click(&mut cx, "reference-row-0");
    click(&mut cx, "reference-row-1");
    click(&mut cx, "reference-row-0");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .options
            .references
            .len()),
        1
    );
    mention(&shell, &mut cx);
    cx.simulate_keystrokes("escape");
    frame(&mut cx);
    assert!(cx.debug_bounds("reference-picker").is_none());
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        "@docs @docs @"
    );
    cx.simulate_input("f");
    frame(&mut cx);
    cx.simulate_keystrokes("backspace");
    frame(&mut cx);
    assert!(cx.debug_bounds("reference-picker").is_some());
    cx.simulate_keystrokes("escape");
    frame(&mut cx);
    cx.simulate_keystrokes("secondary-a backspace");
    frame(&mut cx);
    assert!(cx.update(|_, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .read(cx)
            .tokens()
            .is_empty()
    }));
}

#[gpui::test]
fn anchor_and_empty_results(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [760., 1280.] {
            let handle = cx.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            cx.simulate_window_resize(handle, size(px(width), px(560.)));
            frame(&mut cx);
            mention(&shell, &mut cx);
            let popup = cx.debug_bounds("reference-picker").unwrap();
            let trigger = cx.debug_bounds("composer-toolbar").unwrap();
            assert!(popup.bottom() < trigger.top());
            assert!(popup.top() >= px(0.) && popup.left() >= px(0.) && popup.right() <= px(width));
            let row = cx.debug_bounds("reference-row-0").unwrap();
            assert!(row.left() >= popup.left() && row.right() <= popup.right());
            let selected = cx.debug_bounds("reference-selected").unwrap();
            assert!(selected.right() <= row.right() && selected.left() >= row.left());
            cx.simulate_input("not-found");
            frame(&mut cx);
            assert!(cx.debug_bounds("reference-empty").is_some());
            cx.simulate_keystrokes("escape");
            frame(&mut cx);
            cx.simulate_keystrokes("secondary-a backspace");
            frame(&mut cx);
        }
    }
}

#[gpui::test]
fn queues_without_text(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    cx.simulate_input("@files");
    frame(&mut cx);
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].prompt, "@README.md ");
        assert!(matches!(&thread.turns[0].options.as_ref().unwrap().references[0].kind, Kind::File(path) if path == "README.md"));
    });
    cx.simulate_input("@agents");
    frame(&mut cx);
    for _ in 0..3 {
        cx.simulate_keystrokes("enter");
        frame(&mut cx);
    }
    cx.update(|_, cx| {
        let thread = &shell.read(cx).conversations[&(0, 2)];
        assert_eq!(thread.queue.entries.len(), 1);
        let entry = &thread.queue.entries[0];
        assert_eq!(
            entry.text,
            format!(
                "@{} ",
                entry.options.references[0].label.trim_start_matches('@')
            )
        );
        assert!(matches!(
            thread.queue.entries[0].options.references[0].kind,
            Kind::Agent(_)
        ));
        assert!(thread.options.references.is_empty());
    });
    click(&mut cx, "composer-queue");
    click(&mut cx, "queue-edit-1");
    click(&mut cx, "queue-edit-save");
    cx.update(|_, cx| {
        let entry = &shell.read(cx).conversations[&(0, 2)].queue.entries[0];
        assert_eq!(entry.revision, 1);
        assert_eq!(
            entry.text,
            format!(
                "@{} ",
                entry.options.references[0].label.trim_start_matches('@')
            )
        );
        assert_eq!(entry.options.references.len(), 1);
    });
    cx.simulate_keystrokes("escape");
    frame(&mut cx);
    click(&mut cx, "sent-reference-0-0");
    assert_eq!(cx.update(|_, cx| shell.read(cx).page), Page::Conversation);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).side_resource.as_ref().unwrap().page()),
        Page::Files
    );
}

#[gpui::test]
fn scoped_drafts(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    cx.simulate_input("@files");
    frame(&mut cx);
    for _ in 0..2 {
        cx.simulate_keystrokes("enter");
        frame(&mut cx);
    }
    cx.simulate_input("Keep this draft");
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_composer_host(1, window, cx);
            assert!(shell.conversations[&(1, 2)].options.references.is_empty());
            shell.select_composer_host(0, window, cx);
            assert_eq!(shell.conversations[&(0, 2)].options.references.len(), 1);
            let different = shell
                .workspace
                .worktrees
                .iter()
                .find(|(_, worktree)| worktree.project == 0 && worktree.branch.as_ref() != "main")
                .map(|(key, _)| *key)
                .unwrap();
            shell
                .workspace
                .sessions
                .get_mut(&(0, 2))
                .unwrap()
                .owner
                .worktree = different;
            shell.send_preview((0, 2), window, cx);
            assert!(shell.conversations[&(0, 2)].turns.is_empty());
            assert_eq!(
                shell.conversations[&(0, 2)].input.read(cx).value(),
                "@README.md Keep this draft"
            );
        })
    });
    frame(&mut cx);
    crate::feedback::tests::shown(&mut cx);
    assert!(cx.debug_bounds("reference-error").is_none());
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| input.focus(window, cx))
    });
    cx.simulate_keystrokes("secondary-a backspace");
    frame(&mut cx);
    assert!(cx.update(|_, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .read(cx)
            .tokens()
            .is_empty()
    }));
    assert!(cx.debug_bounds("reference-error").is_none());
}

#[gpui::test]
fn preserves_input_selection(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    cx.simulate_input("left right");
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.set_selected_range(5..10, cx);
                input.focus(window, cx);
            })
    });
    mention(&shell, &mut cx);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        "left @"
    );
    click(&mut cx, "reference-row-0");
    click(&mut cx, "reference-row-0");
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].input.read(cx).value()),
        "left @README.md "
    );
    cx.simulate_keystrokes("secondary-a backspace");
    cx.simulate_input("@");
    frame(&mut cx);
    cx.update(|window, cx| {
        shell.read(cx).conversations[&(0, 2)]
            .input
            .clone()
            .update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "\u{4e2d}", Some(1..1), window, cx);
            })
    });
    frame(&mut cx);
    assert!(cx.debug_bounds("reference-picker").is_none());
    cx.simulate_keystrokes("enter");
    frame(&mut cx);
    assert!(cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)].turns.is_empty()));
}

#[gpui::test]
fn bounds_inline_references(cx: &mut TestAppContext) {
    let (shell, mut cx) = draft(cx);
    let handle = cx.update(|window, cx| {
        shell.update(cx, |shell, _| {
            let owner = shell.workspace.sessions[&(0, 2)].owner;
            for index in 0..30 {
                let key = shell.workspace.create_session(owner);
                shell.workspace.sessions.get_mut(&key).unwrap().title =
                    format!("long-session-reference-{index:02}").into();
            }
        });
        window.window_handle()
    });
    cx.simulate_window_resize(handle, size(px(760.), px(560.)));
    frame(&mut cx);
    for index in 0..30 {
        cx.simulate_input("@sessions");
        frame(&mut cx);
        cx.simulate_keystrokes("enter");
        frame(&mut cx);
        cx.simulate_input(&format!("long-session-reference-{index:02}"));
        frame(&mut cx);
        cx.simulate_keystrokes("enter");
        frame(&mut cx);
    }
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .input
            .read(cx)
            .tokens()
            .len()),
        30
    );
    let viewport = cx.debug_bounds("composer-input").unwrap();
    assert!(viewport.size.height <= px(160.), "{viewport:?}");
    assert!(viewport.left() >= px(0.) && viewport.right() <= px(760.));
    cx.simulate_event(ScrollWheelEvent {
        position: viewport.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(10000.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    frame(&mut cx);
    cx.simulate_keystrokes("secondary-a backspace");
    frame(&mut cx);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .input
            .read(cx)
            .tokens()
            .len()),
        0
    );
    cx.simulate_keystrokes("secondary-z");
    frame(&mut cx);
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).conversations[&(0, 2)]
            .input
            .read(cx)
            .tokens()
            .len()),
        30
    );
}
