use super::*;
use core::prelude::v1::test;
use gpui_kit::component::WindowExt as _;

fn labels(menus: &[Menu]) -> Vec<String> {
    let mut labels = Vec::new();
    for menu in menus {
        labels.push(menu.name.to_string());
        for item in &menu.items {
            match item {
                MenuItem::Action { name, .. } => labels.push(name.to_string()),
                MenuItem::SystemMenu(menu) => labels.push(menu.name.to_string()),
                MenuItem::Separator => {}
                MenuItem::Submenu(_) => panic!("unexpected nested application menu"),
            }
        }
    }
    labels
}

fn invoke<A: Action>(visual: &mut VisualTestContext, _: A) {
    visual.cx.update(|cx| {
        let action = cx
            .get_menus()
            .unwrap()
            .into_iter()
            .flat_map(|menu| menu.items)
            .find_map(|item| match item {
                OwnedMenuItem::Action { action, .. } if action.as_any().is::<A>() => Some(action),
                _ => None,
            })
            .expect("registered menu action");
        assert!(cx.is_action_available(action.as_ref()));
        cx.dispatch_action(action.as_ref());
    });
    visual.run_until_parked();
}

fn setup(cx: &mut TestAppContext) -> (Entity<Shell>, VisualTestContext) {
    let (shell, mut visual) = crate::shell::tests::setup(cx);
    visual.update(|window, cx| {
        window.activate_window();
        shell.update(cx, |shell, cx| shell.focus.focus(window, cx));
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (shell, visual)
}

#[test]
fn localizes_every_entry() {
    let english = menus("en", &[]);
    assert_eq!(
        english
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>(),
        ["Sailry", "File", "Edit", "View", "Window"]
    );
    let english = labels(&english);
    let chinese = labels(&menus("zh-CN", &[]));
    assert_eq!(english.len(), chinese.len());
    assert_eq!(english[0], chinese[0]);
    for (english, chinese) in english.iter().zip(&chinese).skip(1) {
        assert_ne!(english, chinese, "untranslated menu entry");
        assert!(!english.starts_with("menu_"));
        assert!(!chinese.starts_with("menu_"));
    }
}

#[test]
fn uses_focused_editing_actions() {
    let menus = menus("en", &[]);
    let actions: Vec<_> = menus[2]
        .items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Action {
                action, os_action, ..
            } => Some((action, os_action)),
            _ => None,
        })
        .collect();
    assert_eq!(actions.len(), 6);
    for ((action, os_action), expected) in actions.into_iter().zip([
        (input::Undo.boxed_clone(), OsAction::Undo),
        (input::Redo.boxed_clone(), OsAction::Redo),
        (input::Cut.boxed_clone(), OsAction::Cut),
        (input::Copy.boxed_clone(), OsAction::Copy),
        (input::Paste.boxed_clone(), OsAction::Paste),
        (input::SelectAll.boxed_clone(), OsAction::SelectAll),
    ]) {
        assert_eq!(action.as_any().type_id(), expected.0.as_any().type_id());
        assert!(*os_action == Some(expected.1));
    }
    assert!(menus[0].items.iter().any(|item| matches!(item, MenuItem::SystemMenu(menu) if menu.menu_type == SystemMenuType::Services)));
    assert!(menus[0].items.iter().any(|item| matches!(item, MenuItem::Action {action,..} if action.as_any().is::<crate::shell::Quit>())));
}

#[gpui::test]
fn project_workflows(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let count = shell.read_with(&visual, |shell, _| shell.workspace.projects.len());
    invoke(&mut visual, NewProject);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("project-editor").is_some());
    assert!(visual.debug_bounds("directory-picker").is_none());
    assert_eq!(
        shell.read_with(&visual, |shell, _| shell.workspace.projects.len()),
        count
    );
    visual.update(|window, cx| window.close_dialog(cx));
    invoke(&mut visual, OpenFolder);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("directory-picker").is_some());
    assert!(visual.debug_bounds("project-editor").is_none());
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let confirm = visual.debug_bounds("directory-confirm").unwrap();
    visual.simulate_click(confirm.center(), Modifiers::default());
    visual.run_until_parked();
    assert!(shell.read_with(&visual, |shell, _| {
        shell
            .workspace
            .projects
            .values()
            .any(|project| project.path == "/preview")
    }));
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn unpinned_destinations(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.update(|window, cx| {
        crate::preferences::update(cx, |data| {
            data.feature_pins
                .get_or_insert_default()
                .insert("page:files".into(), false);
        });
        cx.refresh_windows();
        window.draw(cx).clear(cx);
    });
    assert!(visual.debug_bounds("navigation-files").is_none());
    let expected = shell.read_with(&visual, |shell, cx| shell.application_navigation(cx));
    visual.cx.update(|cx| {
        let view = cx.get_menus().unwrap().remove(3);
        let entries: Vec<_> = view
            .items
            .into_iter()
            .filter_map(|item| match item {
                OwnedMenuItem::Action { action, name, .. } => action
                    .as_any()
                    .downcast_ref::<OpenFeature>()
                    .map(|action| (action.0.clone(), name)),
                _ => None,
            })
            .collect();
        assert_eq!(
            entries,
            expected
                .into_iter()
                .map(|entry| (entry.key, entry.label.to_string()))
                .collect::<Vec<_>>()
        );
        let action = OpenFeature("page:files".into());
        assert!(cx.is_action_available(&action));
        cx.dispatch_action(&action);
    });
    visual.run_until_parked();
    assert_eq!(shell.read_with(&visual, |shell, _| shell.page), Page::Files);
}

#[gpui::test]
fn follows_focus_and_preserves_drafts(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let input = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.focus.focus(window, cx);
            // Root keeps Copy available for read-only text selections.
            assert!(!window.is_action_available(&input::Cut, cx));
            let input = shell.conversations[&(0, 0)].input.clone();
            input.update(cx, |input, cx| {
                input.set_value("Retained draft", window, cx);
                input.focus(window, cx);
            });
            input
        })
    });
    visual.run_until_parked();
    invoke(&mut visual, input::SelectAll);
    invoke(&mut visual, input::Copy);
    assert_eq!(
        visual.read_from_clipboard().unwrap().text().as_deref(),
        Some("Retained draft")
    );
    invoke(&mut visual, input::Cut);
    assert_eq!(
        input.read_with(&visual, |input, _| input.value().to_string()),
        ""
    );
    invoke(&mut visual, input::Undo);
    assert_eq!(
        input.read_with(&visual, |input, _| input.value().to_string()),
        "Retained draft"
    );
    invoke(&mut visual, input::Redo);
    assert_eq!(
        input.read_with(&visual, |input, _| input.value().to_string()),
        ""
    );
    invoke(&mut visual, input::Paste);
    assert_eq!(
        input.read_with(&visual, |input, _| input.value().to_string()),
        "Retained draft"
    );
    invoke(&mut visual, About);
    shell.read_with(&visual, |shell, cx| {
        assert_eq!(shell.page, Page::Settings);
        assert_eq!(shell.settings.read(cx).section, Section::About);
        assert_eq!(
            shell.conversations[&(0, 0)].input.read(cx).value(),
            "Retained draft"
        );
    });
}

#[gpui::test]
fn creates_preview_in_selected_workspace(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let before = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.select_host(1, window, cx);
            shell.conversations[&(1, 0)].input.update(cx, |input, cx| {
                input.set_value("Existing draft", window, cx)
            });
            (shell.workspace.owner(1), shell.workspace.sessions.len())
        })
    });
    visual.run_until_parked();
    visual.simulate_keystrokes("cmd-n");
    shell.read_with(&visual, |shell, cx| {
        assert!(shell.live.is_none());
        assert!(!cx.has_global::<crate::backend::Services>());
        assert_eq!(shell.page, Page::Conversation);
        assert_eq!(shell.host, 1);
        assert_ne!(shell.session, 0);
        assert_eq!(shell.workspace.sessions.len(), before.1 + 1);
        assert_eq!(
            shell.workspace.sessions[&(shell.host, shell.session)].owner,
            before.0
        );
        assert_eq!(
            shell.conversations[&(1, 0)].input.read(cx).value(),
            "Existing draft"
        );
    });
}

#[gpui::test]
fn refreshes_after_shortcut_changes(cx: &mut TestAppContext) {
    let (_, visual) = setup(cx);
    visual.cx.update(|cx| {
        for (id, action, key) in [
            ("conversation.new", NewConversation.boxed_clone(), "cmd-n"),
            ("window.close", CloseWindow.boxed_clone(), "cmd-shift-w"),
            ("app.hide", Hide.boxed_clone(), "cmd-h"),
            ("app.hide_others", HideOthers.boxed_clone(), "cmd-alt-h"),
            ("window.minimize", Minimize.boxed_clone(), "cmd-m"),
            ("window.full_screen", FullScreen.boxed_clone(), "ctrl-cmd-f"),
        ] {
            assert!(crate::shortcuts::key(id, cx).is_none());
            assert_eq!(
                crate::shortcuts::save(id, Some("cmd-alt-shift-j"), cx),
                Err("shortcut_unavailable")
            );
            assert_eq!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(action.as_ref())
                    .next()
                    .unwrap()
                    .keystrokes()[0]
                    .as_keystroke(),
                &Keystroke::parse(key).unwrap()
            );
            assert!(cx.is_action_available(action.as_ref()));
        }
        cx.set_menus([Menu::new("Stale")]);
        crate::shortcuts::save("app.search", Some("cmd-alt-shift-j"), cx).unwrap();
        assert_eq!(cx.get_menus().unwrap().len(), 5);
        let bindings = cx.key_bindings();
        let bindings = bindings.borrow();
        let keys = bindings
            .bindings_for_action(&crate::shell::Search)
            .map(|binding| binding.keystrokes()[0].as_keystroke().clone())
            .collect::<Vec<_>>();
        assert_eq!(keys, [Keystroke::parse("cmd-alt-shift-j").unwrap()]);
    });
}

#[gpui::test]
fn closes_only_the_active_window(cx: &mut TestAppContext) {
    let (_, mut first) = setup(cx);
    let first_handle = first.update(|window, _| window.window_handle());
    let (_, second) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::secondary(window, cx));
        gpui_kit::component::Root::new(shell, window, cx)
    });
    let second_handle = second.update(|window, _| window.window_handle());
    first.update(|window, _| window.activate_window());
    first.run_until_parked();
    invoke(&mut first, CloseWindow);
    cx.update(|cx| {
        assert!(!cx.windows().contains(&first_handle));
        assert!(cx.windows().contains(&second_handle));
    });
    assert!(!cx.has_pending_prompt());
}
