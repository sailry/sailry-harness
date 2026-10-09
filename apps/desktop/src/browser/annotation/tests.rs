use super::*;
use core::prelude::v1::test;

fn element() -> Element {
    Element {
        url: "https://example.com/page".into(),
        selector: "#heading".into(),
        text: "Visible heading".into(),
        bounds: [10., 20., 180., 30.],
        viewport: [640., 480.],
        truncated: false,
    }
}

#[test]
fn retains_exact_context_and_optional_note() {
    let element = element();
    let value: serde_json::Value =
        serde_json::from_str(&element.reference("Replace this heading").unwrap()).unwrap();
    assert_eq!(value["page_element"]["selector"], "#heading");
    assert_eq!(value["page_element"]["text"], "Visible heading");
    assert_eq!(
        value["page_element"]["bounds"],
        serde_json::json!([10., 20., 180., 30.])
    );
    assert_eq!(value["annotation"], "Replace this heading");
    assert!(element.reference("").is_ok());
}

#[test]
fn rejects_unbounded_or_unusable_context() {
    let mut item = element();
    item.text = "x".repeat(16 * 1024 + 1);
    assert!(!item.validate());
    item = element();
    item.bounds[0] = f64::NAN;
    assert!(!item.validate());
    item = element();
    item.bounds[1] = 900.;
    assert!(!item.validate());
    item = element();
    item.url = "https://user:secret@example.com/".into();
    assert!(!item.validate());
    item = element();
    item.selector.clear();
    assert!(!item.validate());
    assert!(element().reference(&"x".repeat(4097)).is_err());
}

#[test]
fn cancels_pending_selection_when_released() {
    let stop = CancellationToken::new();
    let state = State {
        target: Some(Target {
            token: "selected".into(),
            tab: 0,
            url: element().url,
            lease: 0,
            stop: stop.clone(),
        }),
        task: None,
        captured: false,
    };
    drop(state);
    assert!(stop.is_cancelled());
}

#[gpui_kit::test]
fn invalidates_selection_on_view_or_tab_change(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        Root::new(browser, window, cx)
    });
    visual.update(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        browser.update(cx, |browser, cx| {
            browser.tabs[0].url = element().url;
            let target = Target {
                token: "selected".into(),
                tab: 0,
                url: browser.tabs[0].url.clone(),
                lease: browser.lease,
                stop: CancellationToken::new(),
            };
            browser.annotation.target = Some(target.clone());
            assert!(browser.annotation_matches(&target));
            browser.visibility(false, window, cx);
            assert!(target.stop.is_cancelled());
            assert!(!browser.annotation.active());
            browser.visibility(true, window, cx);
            let next = Target {
                stop: CancellationToken::new(),
                ..target
            };
            browser.annotation.target = Some(next.clone());
            browser.add(window, cx);
            assert!(next.stop.is_cancelled());
            assert!(!browser.annotation.active());
        });
    });
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[gpui_kit::test]
fn ignores_other_documents_and_selection_tokens(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        Root::new(browser, window, cx)
    });
    visual.update(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        browser.update(cx, |browser, cx| {
            browser.tabs[0].url = element().url;
            browser.annotation.target = Some(Target {
                token: "current".into(),
                tab: 0,
                url: browser.tabs[0].url.clone(),
                lease: 0,
                stop: CancellationToken::new(),
            });
            browser.select_annotation(
                0,
                Message::Cancelled {
                    token: "old".into(),
                },
                window,
                cx,
            );
            browser.select_annotation(
                1,
                Message::Cancelled {
                    token: "current".into(),
                },
                window,
                cx,
            );
            assert!(browser.annotation.active());
            browser.tabs[0].url = "https://other.example/".into();
            browser.select_annotation(
                0,
                Message::Cancelled {
                    token: "current".into(),
                },
                window,
                cx,
            );
            assert!(browser.annotation.active());
            browser.tabs[0].url = element().url;
            browser.select_annotation(
                0,
                Message::Cancelled {
                    token: "current".into(),
                },
                window,
                cx,
            );
            assert!(!browser.annotation.active());
        });
    });
}
