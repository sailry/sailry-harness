use super::*;
use crate::preview::Page;
use core::prelude::v1::test;

#[gpui::test]
fn persists_owned_bindings(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::preferences::Preferences::open(path.clone()));
        crate::shell::shortcuts::init(cx);
        init(cx);
        let count = cx.key_bindings().borrow().bindings().len();
        save("app.search", Some("secondary-shift-j"), cx).unwrap();
        assert_eq!(key("app.search", cx).as_deref(), Some("secondary-shift-j"));
        assert_eq!(cx.key_bindings().borrow().bindings().len(), count);
        assert!(renderer_conflict("secondary-shift-j", cx));
        assert!(renderer_conflict("secondary-shift-j secondary-p", cx));
        for key in ["secondary-p", "ctrl-`", "ctrl-shift-g", "secondary-t"] {
            assert!(!renderer_conflict(key, cx), "{key}");
        }
        let search = cx
            .key_bindings()
            .borrow()
            .bindings_for_action(&shell::Search)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].keystrokes()[0].as_keystroke().key, "j");
        assert_eq!(
            crate::preferences::Preferences::open(path)
                .data
                .shortcuts
                .unwrap()["app.search"],
            "secondary-shift-j"
        );
        assert_eq!(
            save("app.search", Some("secondary-q"), cx),
            Err("shortcut_conflict")
        );
        assert_eq!(
            save("panel.toggle", Some("secondary-w"), cx),
            Err("shortcut_conflict")
        );
        save("app.search", Some(""), cx).unwrap();
        assert!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&shell::Search)
                .next()
                .is_none()
        );
        save("app.search", None, cx).unwrap();
        assert_eq!(key("app.search", cx).as_deref(), Some("secondary-k"));
        assert_eq!(cx.key_bindings().borrow().bindings().len(), count);
    });
}

#[test]
fn validates_sequences() {
    for os in [
        OperatingSystem::Mac,
        OperatingSystem::Linux,
        OperatingSystem::Windows,
    ] {
        assert!(valid("secondary-k", os));
        assert!(valid("ctrl-g down", os));
        assert!(valid("", os));
        assert!(!valid(" ", os));
        assert!(!valid("ctrl-g down up", os));
        assert!(!valid("unknownmod-foo", os));
    }
}

#[gpui::test]
fn stale_navigation_overrides_are_inert(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(std::collections::BTreeMap::from([
            ("hints.workspace".into(), "secondary-w".into()),
            ("navigate.files".into(), "secondary-f".into()),
            ("settings_hints.skills".into(), "secondary-s".into()),
            ("file.save".into(), "secondary-j".into()),
            ("app.quit".into(), "secondary-j".into()),
            ("git.commit".into(), "secondary-j".into()),
        ]));
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        init(cx);
        assert!(entries(cx).iter().all(|entry| !entry.id.contains("hints.")
            && (!entry.id.starts_with("navigate.") || entry.id == "navigate.settings")));
        assert_eq!(
            entries(cx).iter().map(|entry| entry.id).collect::<Vec<_>>(),
            [
                "conversation.new",
                "project.new",
                "folder.open",
                "app.search",
                "sidebar.toggle",
                "panel.toggle",
                "host.refresh",
                "conversation.previous",
                "conversation.next",
                "app.shortcuts",
                "system.activate",
                "system.selection"
            ]
        );
        for id in [
            "pane.close",
            "file.save",
            "conversation.find",
            "app.quit",
            "git.commit",
        ] {
            assert!(key(id, cx).is_none());
            assert_eq!(
                save(id, Some("secondary-j"), cx),
                Err("shortcut_unavailable")
            );
        }
        let bindings = cx.key_bindings();
        let bindings = bindings.borrow();
        assert_eq!(
            bindings
                .bindings_for_action(&crate::resources::SaveFile)
                .next()
                .unwrap()
                .keystrokes()[0]
                .as_keystroke(),
            &Keystroke::parse_with_os("secondary-s", cx.platform_info().os).unwrap()
        );
        assert_eq!(
            bindings
                .bindings_for_action(&shell::Quit)
                .next()
                .unwrap()
                .keystrokes()[0]
                .as_keystroke(),
            &Keystroke::parse_with_os("secondary-q", cx.platform_info().os).unwrap()
        );
        assert!(
            bindings
                .bindings_for_action(&OpenPage(Page::Conversation))
                .next()
                .is_none()
        );
    });
}

#[gpui::test]
fn defaults_include_cross_platform_commands(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::preferences::Preferences::default());
        crate::shell::shortcuts::init(cx);
        init(cx);
        for (id, expected) in [
            ("conversation.new", "secondary-n"),
            ("project.new", "secondary-alt-n"),
            ("folder.open", "secondary-o"),
            ("panel.toggle", "secondary-alt-b"),
            ("host.refresh", "secondary-shift-r"),
            ("conversation.previous", "secondary-shift-["),
            ("conversation.next", "secondary-shift-]"),
            ("app.shortcuts", "secondary-/"),
            ("system.activate", "ctrl-alt-space"),
            ("system.selection", ""),
        ] {
            assert_eq!(key(id, cx).as_deref(), Some(expected));
            for os in [
                OperatingSystem::Mac,
                OperatingSystem::Windows,
                OperatingSystem::Linux,
            ] {
                assert!(valid(expected, os));
            }
        }
        for action in [
            NewConversation.boxed_clone(),
            NewProject.boxed_clone(),
            OpenFolder.boxed_clone(),
        ] {
            assert_eq!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(action.as_ref())
                    .count(),
                1
            );
        }
        let (split_right, split_down) = if cx.platform_info().os == OperatingSystem::Mac {
            ("secondary-d", "secondary-shift-d")
        } else {
            ("ctrl-shift-d", "ctrl-shift-alt-d")
        };
        for key in [
            "secondary-n",
            "secondary-alt-n",
            "secondary-o",
            "secondary-alt-b",
            "secondary-shift-r",
            "secondary-shift-[",
            "secondary-shift-]",
            "secondary-/",
            "secondary-.",
            "secondary-l",
            "secondary-shift-m",
            "shift-tab",
            "ctrl-shift-`",
            split_right,
            split_down,
        ] {
            assert_eq!(
                save("app.search", Some(key), cx),
                Err("shortcut_conflict"),
                "{key}"
            );
        }
        assert_eq!(
            save("system.activate", Some("secondary-n"), cx),
            Err("shortcut_conflict")
        );
    });
}

#[gpui::test]
fn preserves_existing_overrides(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let overrides = std::collections::BTreeMap::from([
            ("panel.toggle".into(), "secondary-shift-b".into()),
            ("host.refresh".into(), "secondary-r".into()),
            ("system.activate".into(), "secondary-alt-space".into()),
            ("system.selection".into(), "secondary-alt-s".into()),
        ]);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(overrides.clone());
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        init(cx);
        for (id, expected) in overrides {
            assert_eq!(key(&id, cx), Some(expected));
        }
    });
}

#[gpui::test]
fn existing_override_precedes_new_default(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(std::collections::BTreeMap::from([(
            "app.search".into(),
            "secondary-alt-b".into(),
        )]));
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        init(cx);
        assert_eq!(key("app.search", cx).as_deref(), Some("secondary-alt-b"));
        assert_eq!(key("panel.toggle", cx).as_deref(), Some(""));
        assert_eq!(save("panel.toggle", None, cx), Err("shortcut_conflict"));
        assert_eq!(
            crate::preferences::data(cx).shortcuts.unwrap()["app.search"],
            "secondary-alt-b"
        );
        assert!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&shell::ToggleDetails)
                .next()
                .is_none()
        );
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&shell::Search)
                .count(),
            1
        );
        save("app.search", None, cx).unwrap();
        save("panel.toggle", None, cx).unwrap();
        assert_eq!(key("panel.toggle", cx).as_deref(), Some("secondary-alt-b"));
    });
}

#[gpui::test]
fn restores_fixed_binding_after_override_reset(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(std::collections::BTreeMap::from([(
            "app.search".into(),
            "secondary-l".into(),
        )]));
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        init(cx);
        let focus = crate::conversation::live::shortcuts::FocusComposer;
        assert_eq!(key("app.search", cx).as_deref(), Some("secondary-l"));
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&focus)
                .count(),
            0
        );
        save("app.search", Some("secondary-l"), cx).unwrap();
        save("app.search", None, cx).unwrap();
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&focus)
                .count(),
            1
        );
    });
}

#[test]
fn preserves_terminal_control_keys() {
    let terminal = [
        KeyContext::parse("Sailry").unwrap(),
        KeyContext::parse("SailryTerminal").unwrap(),
    ];
    for os in [
        OperatingSystem::Mac,
        OperatingSystem::Windows,
        OperatingSystem::Linux,
    ] {
        let predicate = KeyBindingContextPredicate::parse(application_context(os)).unwrap();
        assert_eq!(predicate.depth_of(&terminal[..1]), Some(1));
        assert_eq!(
            predicate.depth_of(&terminal),
            (os == OperatingSystem::Mac).then_some(1)
        );
    }
}

#[gpui::test]
fn mode_cycle_requires_composer_context(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::conversation::live::shortcuts::init(cx);
        let bindings = cx.key_bindings();
        let bindings = bindings.borrow();
        let binding = bindings
            .bindings_for_action(&crate::conversation::live::shortcuts::CycleMode)
            .next()
            .unwrap();
        let predicate = binding.predicate().unwrap();
        let question = [
            KeyContext::parse("LiveConversation").unwrap(),
            KeyContext::parse("Input").unwrap(),
        ];
        assert_eq!(predicate.depth_of(&question), None);
        let composer = [
            question[0].clone(),
            KeyContext::parse("ConversationComposer").unwrap(),
            question[1].clone(),
        ];
        assert_eq!(predicate.depth_of(&composer), Some(2));
    });
}

#[gpui::test]
fn renderer_conflicts_follow_platform_bindings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::preferences::Preferences::default());
        crate::shell::shortcuts::init(cx);
        init(cx);
        assert!(renderer_conflict("secondary-n", cx));
        assert!(renderer_conflict("secondary-l", cx));
        assert!(renderer_conflict("shift-tab", cx));
        assert_eq!(
            renderer_conflict("ctrl-n", cx),
            cx.platform_info().os != OperatingSystem::Mac
        );
    });
}

#[gpui::test]
fn installed_resource_remains_available(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::preferences::Preferences::default());
        crate::shell::shortcuts::init(cx);
        init(cx);
        let shortcut = "secondary-t";
        assert!(!renderer_conflict(shortcut, cx));
        let bindings = crate::ui::keys::Bindings::new(cx);
        bindings.update(cx, |bindings, cx| {
            bindings.replace(
                vec![
                    KeyBinding::load(
                        shortcut,
                        Box::new(crate::resources::shortcuts::OpenResource(
                            sailry_protocol::plugin::desktop::ResourceKind::Browser,
                        )),
                        Some(
                            KeyBindingContextPredicate::parse(&bindings.context())
                                .unwrap()
                                .into(),
                        ),
                        false,
                        None,
                        cx,
                    )
                    .unwrap(),
                ],
                cx,
            );
        });
        assert!(!renderer_conflict(shortcut, cx));
        save("app.search", Some("secondary-alt-j"), cx).unwrap();
        assert!(!renderer_conflict(shortcut, cx));
        assert!(renderer_conflict("secondary-l", cx));
    });
}
