use super::*;
use core::prelude::v1::test;

pub(super) fn fixture() -> Browser {
    Browser {
        tabs: vec![Tab::empty(0)],
        #[cfg(target_os = "macos")]
        store: None,
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
        assert_eq!(close["strokes"][0]["meta"], true);
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
