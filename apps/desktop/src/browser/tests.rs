use super::*;
use core::prelude::v1::test;

pub(super) fn fixture() -> Browser {
    Browser {
        tabs: vec![Tab::empty(0)],
        #[cfg(target_os = "macos")]
        store: None,
        #[cfg(target_os = "macos")]
        persistent: false,
        reload: false,
        selected: 0,
        serial: 1,
        changes: tokio::sync::watch::channel(serde_json::Value::Null).0,
        cursor: 0,
        lease: 0,
        mounted: Rc::new(Cell::new(true)),
        stop: sailry_link::CancellationToken::new(),
        preview: false,
        annotation: annotation::State::default(),
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        events: tokio::sync::mpsc::unbounded_channel().0,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        _events: Task::ready(()),
    }
}

mod lifecycle {
    use super::*;

    #[test]
    fn cancellation_and_scope() {
        for (scenario, check) in [
            (
                "closure_cancels_work_and_clears_tabs",
                (|_scenario: &str| {
                    let mut browser = fixture();
                    browser.tabs.push(Tab::empty(1));
                    browser.selected = 1;
                    browser.reload = true;
                    let stop = browser.stop.clone();
                    let lease = browser.lease;
                    browser.close_all();
                    assert!(stop.is_cancelled(), "closure cancels work and clears tabs");
                    assert!(
                        browser.tabs.is_empty(),
                        "closure cancels work and clears tabs"
                    );
                    assert!(
                        !browser.mounted.get(),
                        "closure cancels work and clears tabs"
                    );
                    assert!(!browser.reload, "closure cancels work and clears tabs");
                    assert_eq!(browser.selected, 0, "closure cancels work and clears tabs");
                    assert!(
                        browser.lease > lease,
                        "closure cancels work and clears tabs"
                    );
                    browser.close_all();
                    assert!(
                        browser.tabs.is_empty(),
                        "closure cancels work and clears tabs"
                    );
                    assert!(stop.is_cancelled(), "closure cancels work and clears tabs");
                }) as fn(&str),
            ),
            (
                "dropping_cancels_work",
                (|_scenario: &str| {
                    let browser = fixture();
                    let stop = browser.stop.clone();
                    assert!(!stop.is_cancelled(), "dropping cancels work");
                    drop(browser);
                    assert!(stop.is_cancelled(), "dropping cancels work");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

#[test]
fn cancellation_and_scope() {
    for (scenario, check) in [
        (
            "destinations",
            (|_scenario: &str| {
                assert_eq!(
                    destination(" https://example.com "),
                    Some("https://example.com/".into()),
                    "destinations"
                );
                assert_eq!(
                    destination("http://localhost:8080/a"),
                    Some("http://localhost:8080/a".into()),
                    "destinations"
                );
                assert_eq!(
                    destination("https://example.com/path?q=a"),
                    Some("https://example.com/path?q=a".into()),
                    "destinations"
                );
                for invalid in [
                    "",
                    "example.com",
                    "localhost:8080/a",
                    "two words",
                    "/relative/path",
                    "javascript:alert(1)",
                    "file:///tmp/file",
                    "data:text/html,test",
                ] {
                    assert!(destination(invalid).is_none(), "destinations");
                }
            }) as fn(&str),
        ),
        (
            "controller_actions_cannot_choose_another_scope",
            (|_scenario: &str| {
                for kind in ["add", "back", "forward", "reload", "stop"] {
                    assert!(
                        serde_json::from_value::<state::Action>(serde_json::json!({"kind":kind}))
                            .is_ok(),
                        "controller actions cannot choose another scope"
                    );
                    assert!(
                        serde_json::from_value::<state::Action>(
                            serde_json::json!({"kind":kind,"session":"another"})
                        )
                        .is_err(),
                        "controller actions cannot choose another scope"
                    );
                }
                for arguments in [
                    serde_json::json!({"kind":"navigate","url":"https://example.com","node":"another"}),
                    serde_json::json!({"kind":"select","id":1,"worktree":"another"}),
                ] {
                    assert!(
                        serde_json::from_value::<state::Action>(arguments).is_err(),
                        "controller actions cannot choose another scope"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[gpui::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn fixed_shortcuts_reach_webviews(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shell::shortcuts::init(cx);
        shortcuts::init(cx);
        shortcuts::init(cx);
        crate::shortcuts::init(cx);
        let script = shortcuts::script(cx);
        let source = script
            .strip_prefix("(() => { const bindings = ")
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let bindings: serde_json::Value = serde_json::from_str(source).unwrap();
        let close = bindings
            .as_array()
            .unwrap()
            .iter()
            .find(|binding| binding["action"] == "close-tab")
            .unwrap();
        assert_eq!(close["strokes"][0]["key"], "w");
        assert_eq!(close["strokes"][0]["meta"], cfg!(target_os = "macos"));
        assert_eq!(close["strokes"][0]["ctrl"], !cfg!(target_os = "macos"));
        for (action, key) in [("new-tab", "t"), ("reload", "r")] {
            let binding = bindings
                .as_array()
                .unwrap()
                .iter()
                .find(|binding| binding["action"] == action)
                .unwrap();
            assert_eq!(binding["strokes"][0]["key"], key);
            assert_eq!(binding["strokes"][0]["meta"], cfg!(target_os = "macos"));
            assert_eq!(binding["strokes"][0]["ctrl"], !cfg!(target_os = "macos"));
        }
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&shortcuts::NewBrowserTab)
                .count(),
            1
        );
        #[cfg(target_os = "macos")]
        {
            let inspect = bindings
                .as_array()
                .unwrap()
                .iter()
                .find(|binding| binding["action"] == "inspect")
                .unwrap();
            assert_eq!(inspect["strokes"][0]["key"], "i");
            assert_eq!(inspect["strokes"][0]["meta"], true);
            assert_eq!(inspect["strokes"][0]["shift"], true);
            assert_eq!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(&inspector::Inspect)
                    .count(),
                1
            );
        }
        crate::shortcuts::save("app.search", Some("secondary-alt-shift-j"), cx).unwrap();
        assert_eq!(shortcuts::script(cx), script);
    });
}

#[gpui::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn refresh_override_before_browser(cx: &mut TestAppContext) {
    refresh_override(false, cx);
}

#[gpui::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn refresh_override_after_browser(cx: &mut TestAppContext) {
    refresh_override(true, cx);
}

#[gpui::test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn existing_override_survives_initialization(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(std::collections::BTreeMap::from([(
            "app.search".into(),
            "secondary-t".into(),
        )]));
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        crate::shortcuts::init(cx);
        for _ in 0..2 {
            shortcuts::init(cx);
            assert_eq!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(&shortcuts::NewBrowserTab)
                    .count(),
                0
            );
            assert!(!shortcuts::script(cx).contains("\"action\":\"new-tab\""));
        }
        crate::shortcuts::save("app.search", Some("secondary-t"), cx).unwrap();
        shortcuts::init(cx);
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&shortcuts::NewBrowserTab)
                .count(),
            0
        );
        crate::shortcuts::save("app.search", None, cx).unwrap();
        for _ in 0..2 {
            shortcuts::init(cx);
            assert_eq!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(&shortcuts::NewBrowserTab)
                    .count(),
                1
            );
            assert!(shortcuts::script(cx).contains("\"action\":\"new-tab\""));
        }
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn refresh_override(browser_first: bool, cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut preferences = crate::preferences::Preferences::default();
        preferences.data.shortcuts = Some(std::collections::BTreeMap::from([(
            "host.refresh".into(),
            "secondary-r".into(),
        )]));
        cx.set_global(preferences);
        crate::shell::shortcuts::init(cx);
        if browser_first {
            shortcuts::init(cx);
        }
        crate::shortcuts::init(cx);
        if !browser_first {
            shortcuts::init(cx);
        }
        crate::shortcuts::init(cx);
        assert_eq!(
            crate::shortcuts::key("host.refresh", cx).as_deref(),
            Some("secondary-r")
        );
        crate::shortcuts::save("host.refresh", Some("secondary-r"), cx).unwrap();
        assert_eq!(
            crate::shortcuts::save("system.activate", Some("secondary-r"), cx),
            Err("shortcut_conflict")
        );
        let input = [Keystroke::parse_with_os("secondary-r", cx.platform_info().os).unwrap()];
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, pending) =
            keymap.bindings_for_input(&input, &[KeyContext::parse("Sailry").unwrap()]);
        assert!(!pending);
        assert!(
            bindings[0]
                .action()
                .as_any()
                .is::<crate::shell::shortcuts::RefreshHost>()
        );
        let (bindings, pending) = keymap.bindings_for_input(
            &input,
            &[
                KeyContext::parse("Sailry").unwrap(),
                KeyContext::parse("SailryBrowser").unwrap(),
            ],
        );
        assert!(!pending);
        assert!(
            bindings[0]
                .action()
                .as_any()
                .is::<shortcuts::ReloadBrowser>()
        );
        assert_eq!(
            crate::preferences::data(cx).shortcuts.unwrap()["host.refresh"],
            "secondary-r"
        );
    });
}
