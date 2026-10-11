use super::*;
use crate::ui::webview_capture::Buffer;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::io::Cursor;

struct Harness;
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn buffer() -> Buffer {
    let bitmap = image::DynamicImage::new_rgba8(2, 2);
    let mut bytes = Cursor::new(Vec::new());
    bitmap
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Buffer {
        bytes: bytes.into_inner(),
        format: image::ImageFormat::Png,
    }
}

fn color(rgba: [u8; 4]) -> Buffer {
    let bitmap =
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba)));
    let mut bytes = Cursor::new(Vec::new());
    bitmap
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Buffer {
        bytes: bytes.into_inner(),
        format: image::ImageFormat::Png,
    }
}

fn pixel(snapshot: &Snapshot) -> Vec<u8> {
    snapshot
        .image
        .borrow()
        .as_ref()
        .unwrap()
        .image
        .as_bytes(0)
        .unwrap()[..4]
        .to_vec()
}

#[gpui_kit::test]
fn discards_superseded_completion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness);
        Root::new(view, window, cx)
    });
    let snapshot = Snapshot::new();
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(80.)));
    let (old, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.bind(window, cx);
        snapshot.mount((bounds, bounds));
        snapshot.request(|| Ok(receive), window, cx);
    });
    assert!(snapshot.pending.get());
    assert!(!snapshot.signals().receive("unrelated plugin event"));
    snapshot.signals().receive("sailry-snapshot:dirty");
    snapshot.signals().receive("sailry-snapshot:ready");
    visual.run_until_parked();
    assert!(old.send(Ok(buffer())).is_ok());
    visual.run_until_parked();
    assert!(!snapshot.pending.get());
    assert!(
        snapshot.image.borrow().is_none(),
        "an obsolete capture must not publish its pixels"
    );
    let (latest, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    assert!(latest.send(Ok(buffer())).is_ok());
    visual.run_until_parked();
    assert!(
        snapshot.current(),
        "the settled generation must accept its own completion"
    );
    snapshot.unmount();
    assert!(!snapshot.shown.get());
    visual.update(|window, _| window.remove_window());
}

#[gpui_kit::test]
fn invalidates_resized_clip(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness);
        Root::new(view, window, cx)
    });
    let snapshot = Snapshot::new();
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(80.)));
    let (send, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.mount((bounds, bounds));
        snapshot.request(|| Ok(receive), window, cx);
    });
    assert!(send.send(Ok(buffer())).is_ok());
    visual.run_until_parked();
    assert!(snapshot.current());
    let moved = Bounds::new(point(px(20.), px(30.)), bounds.size);
    snapshot.mount((moved, moved));
    assert!(
        snapshot.current(),
        "moving the same native viewport preserves its content cache"
    );
    let clip = Bounds::new(moved.origin, size(px(100.), px(60.)));
    snapshot.mount((moved, clip));
    assert!(
        !snapshot.current(),
        "a changed native clip needs a new capture"
    );
    let (stale, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    let narrower = Bounds::new(moved.origin, size(px(80.), px(60.)));
    snapshot.mount((moved, narrower));
    let image = snapshot.image.borrow().as_ref().unwrap().image.id;
    assert!(stale.send(Ok(color([220, 30, 40, 255]))).is_ok());
    visual.run_until_parked();
    assert!(!snapshot.pending.get());
    assert!(!snapshot.current());
    assert_eq!(snapshot.image.borrow().as_ref().unwrap().image.id, image);
    let (latest, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    assert!(latest.send(Ok(color([30, 200, 70, 255]))).is_ok());
    visual.run_until_parked();
    assert!(snapshot.current());
    assert_eq!(pixel(&snapshot), [70, 200, 30, 255]);
    visual.update(|window, _| window.remove_window());
}

#[cfg(target_os = "macos")]
#[gpui_kit::test]
fn captures_before_idle(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness);
        Root::new(view, window, cx)
    });
    let snapshot = Snapshot::new();
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(80.)));
    let (initial, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.bind(window, cx);
        snapshot.mount((bounds, bounds));
        snapshot.refresh(true, || Ok(color([30, 30, 30, 255])), window);
        snapshot.request(|| Ok(receive), window, cx);
        snapshot.request(
            || panic!("an outstanding native capture must be coalesced"),
            window,
            cx,
        );
    });
    assert!(
        !snapshot.current(),
        "matching AppKit pixels do not establish DOM freshness"
    );
    assert_eq!(pixel(&snapshot), [30, 30, 30, 255]);
    assert!(initial.send(Ok(color([220, 30, 40, 255]))).is_ok());
    visual.run_until_parked();
    assert!(snapshot.current());
    assert_eq!(pixel(&snapshot), [40, 30, 220, 255]);
    snapshot.shown.set(true);
    snapshot.signals().receive("sailry-snapshot:dirty");
    visual.run_until_parked();
    assert!(!snapshot.settled.get());
    assert!(!snapshot.current());
    let (changed, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.refresh(
            true,
            || panic!("a matching fallback must not overwrite native snapshots"),
            window,
        );
        snapshot.request(|| Ok(receive), window, cx);
        snapshot.request(
            || panic!("dirty captures must also be coalesced"),
            window,
            cx,
        );
    });
    assert!(snapshot.pending.get(), "capture must not wait for Ready");
    assert!(changed.send(Ok(color([30, 200, 70, 255]))).is_ok());
    visual.run_until_parked();
    assert_eq!(pixel(&snapshot), [70, 200, 30, 255]);
    assert!(snapshot.current());
    assert!(!snapshot.settled.get());
    let (animated, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    assert!(snapshot.pending.get());
    assert!(animated.send(Ok(color([150, 50, 200, 255]))).is_ok());
    visual.run_until_parked();
    assert_eq!(pixel(&snapshot), [200, 50, 150, 255]);
    let image = snapshot.image.borrow().as_ref().unwrap().image.id;
    let (failed, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    assert!(failed.send(Err(())).is_ok());
    visual.run_until_parked();
    assert!(!snapshot.pending.get());
    assert_eq!(snapshot.image.borrow().as_ref().unwrap().image.id, image);
    snapshot.signals().receive("sailry-snapshot:ready");
    visual.run_until_parked();
    let (settled, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| snapshot.request(|| Ok(receive), window, cx));
    assert!(settled.send(Ok(color([30, 200, 70, 255]))).is_ok());
    visual.run_until_parked();
    assert!(snapshot.current());
    visual.update(|window, cx| {
        snapshot.request(
            || panic!("a settled cache must stop recapturing"),
            window,
            cx,
        );
        window.remove_window();
    });
}

#[gpui_kit::test]
fn preserves_fallback_during_resize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness);
        Root::new(view, window, cx)
    });
    let snapshot = Snapshot::new();
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(80.)));
    visual.update(|window, _| {
        snapshot.mount((bounds, bounds));
        snapshot.refresh(true, || Ok(color([220, 30, 40, 255])), window);
    });
    assert!(!snapshot.current());
    snapshot.shown.set(true);
    let clip = Bounds::new(bounds.origin, size(px(100.), px(60.)));
    let (send, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.mount((bounds, clip));
        assert!(!snapshot.current());
        snapshot.refresh(true, || Ok(color([30, 200, 70, 255])), window);
        assert!(!snapshot.current());
        snapshot.request(|| Ok(receive), window, cx);
    });
    assert_eq!(pixel(&snapshot), [70, 200, 30, 255]);
    assert_eq!(snapshot.image.borrow().as_ref().unwrap().frame.1, clip);
    assert!(send.send(Ok(color([150, 50, 200, 255]))).is_ok());
    visual.run_until_parked();
    assert!(snapshot.current());
    assert_eq!(pixel(&snapshot), [200, 50, 150, 255]);
    visual.update(|window, _| {
        snapshot.refresh(false, || panic!("an uncovered view stays native"), window);
        snapshot.shown.set(false);
        snapshot.refresh(
            true,
            || panic!("reopening must preserve the authoritative native image"),
            window,
        );
        assert_eq!(pixel(&snapshot), [200, 50, 150, 255]);
        assert!(snapshot.current());
        window.remove_window();
    });
}

#[gpui_kit::test]
fn accepts_completion_after_fallback(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness);
        Root::new(view, window, cx)
    });
    let snapshot = Snapshot::new();
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(100.), px(80.)));
    let (fresh, receive) = tokio::sync::oneshot::channel();
    visual.update(|window, cx| {
        snapshot.mount((bounds, bounds));
        snapshot.request(|| Ok(receive), window, cx);
        snapshot.refresh(true, || Ok(color([30, 30, 30, 255])), window);
    });
    assert!(snapshot.pending.get());
    assert!(!snapshot.current());
    assert_eq!(pixel(&snapshot), [30, 30, 30, 255]);
    assert!(fresh.send(Ok(color([220, 30, 40, 255]))).is_ok());
    visual.run_until_parked();
    assert!(!snapshot.pending.get());
    assert!(snapshot.current());
    assert_eq!(
        pixel(&snapshot),
        [40, 30, 220, 255],
        "a matching native completion must replace non-authoritative fallback pixels"
    );
    visual.update(|window, _| window.remove_window());
}
