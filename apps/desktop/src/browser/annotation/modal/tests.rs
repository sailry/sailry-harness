use super::*;
use core::prelude::v1::test;
use gpui_kit::{TestAppContext, test::TestWindowExt};
use std::{cell::Cell, rc::Rc};

fn tap(visual: &mut VisualTestContext, id: &'static str) {
    visual.update(|window, cx| {
        window.render_frame(cx);
        let target = window.find(id);
        assert!(target.visible() && target.disabled() != Some(true));
        window.click(id, cx);
    });
    visual.run_until_parked();
}

#[gpui_kit::test]
fn retains_notes_on_failure_and_cancels_cleanly(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let attempts = Rc::new(Cell::new(0));
    let count = attempts.clone();
    let mut browser = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Browser::new(true, window, cx));
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.unwrap();
    let mut entity = None;
    visual.update(|window, cx| {
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut bytes)
            .encode_image(&image::DynamicImage::new_rgb8(12, 8))
            .unwrap();
        let target = Target {
            token: "current".into(),
            tab: 0,
            url: "https://example.com/page".into(),
            lease: 0,
            stop: CancellationToken::new(),
        };
        browser.update(cx, |browser, _| {
            browser.tabs[0].url = target.url.clone();
            browser.annotation.target = Some(target.clone());
        });
        let review = cx.new(|cx| Review {
            browser: browser.downgrade(),
            target,
            element: Element {
                url: "https://example.com/page".into(),
                selector: "#heading".into(),
                text: "Visible heading".into(),
                bounds: [1., 2., 100., 20.],
                viewport: [640., 480.],
                truncated: false,
            },
            preview: Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes.clone())),
            image: bytes.into(),
            note: cx.new(|cx| TextareaState::new(window, cx).default_value("Keep annotation")),
            pending: false,
            closed: false,
            task: None,
        });
        cx.subscribe(&browser, move |_, request: &Transfer, _| {
            count.set(count.get() + 1);
            assert!(request.reference.contains("Keep annotation"));
            if let Some(reply) = request.reply.borrow_mut().take() {
                let _ = reply.send(Err("browser_annotation_stale"));
            }
        })
        .detach();
        entity = Some(review.clone());
        show(review, window, cx);
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let review = entity.unwrap();
    tap(visual, "browser-annotation-add");
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        assert!(window.has_active_dialog(cx));
        assert_eq!(
            review.read(cx).note.read(cx).value().as_ref(),
            "Keep annotation"
        );
        assert!(!review.read(cx).pending);
        assert!(browser.read(cx).annotation.active());
    });
    assert_eq!(attempts.get(), 1);
    tap(visual, "browser-annotation-cancel");
    visual.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert!(!browser.read(cx).annotation.active());
        assert!(review.read(cx).closed);
    });
    assert_eq!(attempts.get(), 1);
}
