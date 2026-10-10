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
fn accepts_only_original_context_and_bounded_note() {
    let item = element();
    let value = serde_json::json!({"page_element":item,"annotation":"Replace this heading"});
    assert!(Reference::matches(&value.to_string(), &item));
    let mut changed = value.clone();
    changed["page_element"]["url"] = "https://other.example/".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed = value.clone();
    changed["page_element"]["text"] = "Forged heading".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed = value.clone();
    changed["annotation"] = "x".repeat(4097).into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed["annotation"] = "\0".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed["annotation"] = "".into();
    assert!(Reference::matches(&changed.to_string(), &item));
    changed["extra"] = true.into();
    assert!(!Reference::matches(&changed.to_string(), &item));
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
}

#[test]
fn cancels_pending_selection_when_released() {
    let stop = CancellationToken::new();
    let mut state = State::default();
    state.target = Some(Target {
        token: "selected".into(),
        tab: 0,
        url: element().url,
        lease: 0,
        stop: stop.clone(),
    });
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

impl Browser {
    pub(crate) fn capture_fixture(&mut self, cx: &mut Context<Self>) -> String {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image::DynamicImage::new_rgb8(12, 8))
            .unwrap();
        self.cancel_annotation(cx);
        let item = element();
        self.tabs[self.selected].url = item.url.clone();
        let token = sailry_protocol::RequestId::new().to_string();
        self.annotation.target = Some(Target {
            token: token.clone(),
            tab: self.tabs[self.selected].id,
            url: item.url.clone(),
            lease: self.lease,
            stop: CancellationToken::new(),
        });
        self.annotation.captured = true;
        self.annotation.capture = Some(Capture {
            element: item,
            preview: Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes.clone())),
            image: bytes.into(),
        });
        self.changed(cx);
        token
    }
}

#[gpui_kit::test]
fn transfers_exact_capture_and_rejects_stale_targets(cx: &mut TestAppContext) {
    use std::{cell::Cell, rc::Rc};
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        Root::new(browser, window, cx)
    });
    let count = Rc::new(Cell::new(0));
    let (browser, token) = visual.update(|window, cx| {
        let browser = cx.new(|cx| Browser::new(true, window, cx));
        let token = browser.update(cx, |browser, cx| browser.capture_fixture(cx));
        let total = count.clone();
        cx.subscribe(&browser, move |_, request: &Transfer, _| {
            total.set(total.get() + 1);
            assert!(!request.image.is_empty());
            if let Some(reply) = request.reply.borrow_mut().take() {
                let _ = reply.send(Ok(()));
            }
        })
        .detach();
        (browser, token)
    });
    let reference =
        serde_json::json!({"page_element":element(),"annotation":"Keep annotation"}).to_string();
    for (selected, value, expected) in [
        (
            "other".to_owned(),
            reference.clone(),
            Err("browser_annotation_stale"),
        ),
        (
            token.clone(),
            "{}".into(),
            Err("browser_annotation_invalid"),
        ),
        (token.clone(), reference.clone(), Ok(())),
    ] {
        let mut receive = visual.update(|_, cx| {
            let (reply, receive) = tokio::sync::oneshot::channel();
            browser.update(cx, |browser, cx| {
                browser.submit_annotation(
                    &Submit {
                        token: selected,
                        reference: value,
                        stop: CancellationToken::new(),
                        reply: RefCell::new(Some(reply)),
                    },
                    cx,
                )
            });
            receive
        });
        visual.run_until_parked();
        assert_eq!(receive.try_recv().unwrap(), expected);
    }
    assert_eq!(count.get(), 1);
    visual.update(|_, cx| {
        browser.update(cx, |browser, cx| {
            browser.cancel_captured_annotation("other", cx);
            assert!(browser.annotation.active());
            browser.acquire_lease(cx);
            assert!(!browser.annotation.active());
            assert!(browser.annotation_snapshot().is_none());
        })
    });
    let mut receive = visual.update(|_, cx| {
        let (reply, receive) = tokio::sync::oneshot::channel();
        browser.update(cx, |browser, cx| {
            browser.submit_annotation(
                &Submit {
                    token,
                    reference,
                    stop: CancellationToken::new(),
                    reply: RefCell::new(Some(reply)),
                },
                cx,
            )
        });
        receive
    });
    visual.run_until_parked();
    assert_eq!(receive.try_recv().unwrap(), Err("browser_annotation_stale"));
    assert_eq!(count.get(), 1);
}

#[gpui_kit::test]
fn covered_surface_preserves_capture_and_checks_lease(cx: &mut TestAppContext) {
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
            let token = browser.capture_fixture(cx);
            let lease = browser.lease;
            browser.cover(lease, true, window, cx);
            assert!(browser.annotation.active());
            assert_eq!(browser.annotation_snapshot().unwrap()["token"], token);
            assert!(browser.annotation_preview(&token).is_some());
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            assert!(!browser.surface_visible());
            browser.cover(lease + 1, false, window, cx);
            assert!(browser.covered);
            browser.cover(lease, false, window, cx);
            assert!(browser.annotation.active());
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            assert!(browser.surface_visible());
            browser.cover(lease, true, window, cx);
            browser.acquire_lease(cx);
            assert!(!browser.covered);
            browser.cover(lease, true, window, cx);
            assert!(!browser.covered);
            assert!(!browser.annotation.active());
        });
    });
}
