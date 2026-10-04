use super::*;
use core::prelude::v1::test;
use gpui_kit::component::input::{Input, InputState};
use std::{cell::Cell, rc::Rc};

struct Tabs {
    browser: Entity<Browser>,
    address: Entity<InputState>,
}

impl Render for Tabs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(Input::new(&self.address))
            .child(self.browser.clone())
    }
}

#[test]
fn destinations() {
    assert_eq!(
        destination(" https://example.com "),
        Some("https://example.com/".into())
    );
    assert_eq!(
        destination("http://localhost:8080/a"),
        Some("http://localhost:8080/a".into())
    );
    assert_eq!(
        destination("https://example.com/path?q=a"),
        Some("https://example.com/path?q=a".into())
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
        assert!(destination(invalid).is_none());
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

#[gpui::test]
fn tab_lifecycle(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        entity = Some(browser.clone());
        Root::new(browser, window, cx)
    });
    let browser = entity.unwrap();
    visual.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            let changes = browser.changes(cx);
            let initial = browser.snapshot(cx);
            assert_eq!(initial["selected"], 0);
            assert_eq!(initial["tabs"][0]["loaded"], false);
            browser.tabs[0].url = "https://example.com/first".into();
            browser.control(state::Action::Add {}, window, cx).unwrap();
            assert_eq!(browser.selected, 1);
            assert_eq!(browser.snapshot(cx)["selected"], 1);
            assert!(browser.tabs[browser.selected].url.is_empty());
            browser.select(0, window, cx);
            assert_eq!(
                browser.snapshot(cx)["tabs"][0]["url"],
                "https://example.com/first"
            );
            browser.close(0, window, cx);
            assert_eq!(browser.tabs.len(), 1);
            assert_eq!(browser.snapshot(cx)["selected"], 1);
            assert_eq!(changes.borrow()["selected"], 1);
            assert!(browser.tabs[0].url.is_empty());
            assert!(
                browser
                    .control(state::Action::Select { id: 0 }, window, cx)
                    .is_err()
            );
            let last = browser.tabs[0].id;
            browser.close(last, window, cx);
            assert!(browser.tabs.is_empty());
            assert_eq!(browser.snapshot(cx)["selected"], serde_json::Value::Null);
            assert_eq!(changes.borrow()["tabs"], serde_json::json!([]));
            browser
                .control(
                    state::Action::Navigate {
                        url: "javascript:alert(1)".into(),
                    },
                    window,
                    cx,
                )
                .unwrap();
            assert_eq!(browser.tabs.len(), 1);
            assert_eq!(
                browser.snapshot(cx)["tabs"][0]["error"],
                "browser_invalid_address"
            );
            assert!(
                browser.snapshot(cx)["cursor"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    > initial["cursor"].as_str().unwrap().parse::<u64>().unwrap()
            );
        });
    });
}

#[gpui::test]
fn empty_tabs_preserve_input_focus(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let dismissed = Rc::new(Cell::new(0));
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        let address = cx.new(|cx| InputState::new(window, cx).default_value("Retained address"));
        let count = dismissed.clone();
        cx.subscribe(&browser, move |_, _, _: &DismissEvent, _| {
            count.set(count.get() + 1);
        })
        .detach();
        entity = Some((browser.clone(), address.clone()));
        let tabs = cx.new(|_| Tabs { browser, address });
        Root::new(tabs, window, cx)
    });
    let (browser, address) = entity.unwrap();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        address.read(cx).focus_handle(cx).focus(window, cx);
        browser.update(cx, |browser, cx| {
            for _ in 0..2 {
                browser.control(state::Action::Add {}, window, cx).unwrap();
            }
            browser
                .control(state::Action::Select { id: 0 }, window, cx)
                .unwrap();
            browser
                .control(state::Action::Close { id: 2 }, window, cx)
                .unwrap();
            assert_eq!(browser.snapshot(cx)["selected"], 0);
            browser
                .control(state::Action::Close { id: 0 }, window, cx)
                .unwrap();
            assert_eq!(browser.snapshot(cx)["selected"], 1);
            assert_eq!(browser.snapshot(cx)["tabs"][0]["id"], 1);
            assert!(
                browser
                    .control(state::Action::Select { id: 0 }, window, cx)
                    .is_err()
            );
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert!(address.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(address.read(cx).value().as_str(), "Retained address");
        browser.update(cx, |browser, cx| {
            browser
                .control(state::Action::Close { id: 1 }, window, cx)
                .unwrap();
            assert!(browser.tabs.is_empty());
        });
    });
    visual.run_until_parked();
    assert_eq!(dismissed.get(), 1);
}

#[test]
fn controller_actions_cannot_choose_another_scope() {
    for kind in ["add", "back", "forward", "reload", "stop"] {
        assert!(serde_json::from_value::<state::Action>(serde_json::json!({"kind":kind})).is_ok());
        assert!(
            serde_json::from_value::<state::Action>(
                serde_json::json!({"kind":kind,"session":"another"})
            )
            .is_err()
        );
    }
    for arguments in [
        serde_json::json!({"kind":"navigate","url":"https://example.com","node":"another"}),
        serde_json::json!({"kind":"select","id":1,"worktree":"another"}),
    ] {
        assert!(serde_json::from_value::<state::Action>(arguments).is_err());
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[gpui::test]
fn empty_tabs_require_navigation(cx: &mut TestAppContext) {
    use sailry_protocol::{ErrorCode, browser::Action};
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        entity = Some(browser.clone());
        Root::new(browser, window, cx)
    });
    let browser = entity.unwrap();
    let task = visual.update(|window, cx| {
        browser.update(cx, |browser, cx| {
            browser.preview = false;
            browser.execute(Action::Read { tab: None }, window, cx)
        })
    });
    let error = visual.foreground_executor().block_on(task).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.message,
        "browser tab is empty; navigate to a URL before reading or interacting"
    );
    assert!(browser.read_with(visual, |browser, _| browser.tabs[0].page.is_none()));
}
