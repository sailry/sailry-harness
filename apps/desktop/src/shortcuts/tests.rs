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
    assert!(valid("secondary-k"));
    assert!(valid("ctrl-g down"));
    assert!(valid(""));
    assert!(!valid(" "));
    assert!(!valid("ctrl-g down up"));
    assert!(!valid("unknownmod-foo"));
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
                "app.search",
                "sidebar.toggle",
                "panel.toggle",
                "host.refresh"
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
            &Keystroke::parse("secondary-s").unwrap()
        );
        assert_eq!(
            bindings
                .bindings_for_action(&shell::Quit)
                .next()
                .unwrap()
                .keystrokes()[0]
                .as_keystroke(),
            &Keystroke::parse("secondary-q").unwrap()
        );
        assert!(
            bindings
                .bindings_for_action(&OpenPage(Page::Conversation))
                .next()
                .is_none()
        );
    });
}
