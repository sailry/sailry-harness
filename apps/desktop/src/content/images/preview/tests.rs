use super::*;
use crate::conversation::live::tests::{
    fixture::{self, Fixture, ImageView, init, tap},
    wait,
};
use core::prelude::v1::test;
use image::{ImageEncoder as _, Rgba, RgbaImage};

fn pixels() -> RgbaImage {
    RgbaImage::from_fn(2, 3, |x, y| Rgba([(y * 2 + x + 1) as u8, 20, 30, 128]))
}

fn values(image: &RenderImage) -> Vec<u8> {
    image
        .as_bytes(0)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            assert_eq!(&pixel[1..], &[20, 30, 128]);
            pixel[0]
        })
        .collect()
}

#[test]
fn transforms_preserve_channels() {
    let image = RenderImage::new([image::Frame::new(pixels())]);
    let rotated = Change::Rotate.apply(&image).unwrap();
    assert_eq!(rotated.size(0), size(DevicePixels(3), DevicePixels(2)));
    assert_eq!(values(&rotated), [5, 3, 1, 6, 4, 2]);
    assert_eq!(
        values(&Change::Horizontal.apply(&image).unwrap()),
        [2, 1, 4, 3, 6, 5]
    );
    assert_eq!(
        values(&Change::Vertical.apply(&image).unwrap()),
        [5, 6, 3, 4, 1, 2]
    );
    let restored = (0..4).fold(Arc::new(image), |image, _| {
        Change::Rotate.apply(&image).unwrap()
    });
    assert_eq!(values(&restored), [1, 2, 3, 4, 5, 6]);
}

fn loaded(preview: &Entity<Preview>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        let view = preview.read(cx);
        assert!(!matches!(view.result, Some(Err(_))), "preview failed");
        matches!(view.result, Some(Ok(_))) && !view.transforming
    });
}

fn sizes_to_viewport(
    preview: &Entity<Preview>,
    visual: &mut VisualTestContext,
) -> gpui_kit::Size<DevicePixels> {
    preview.read_with(visual, |view, _| {
        view.result.as_ref().unwrap().as_ref().unwrap().size(0)
    })
}

fn frame(visual: &mut VisualTestContext, milliseconds: u64) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(milliseconds));
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
}

fn pinch(visual: &mut VisualTestContext, delta: f32) {
    let position = visual
        .debug_bounds("image-lightbox-canvas")
        .unwrap()
        .center();
    visual.simulate_mouse_move(position, None, Modifiers::default());
    visual.simulate_event(PinchEvent {
        position,
        delta,
        phase: TouchPhase::Moved,
        ..Default::default()
    });
    frame(visual, 0);
}

fn drag_to_edge(preview: &Entity<Preview>, visual: &mut VisualTestContext) {
    let canvas = visual.debug_bounds("image-lightbox-canvas").unwrap();
    let image = visual.debug_bounds("image-lightbox-image").unwrap();
    let start = canvas.center();
    let end = point(canvas.right() - px(1.), canvas.bottom() - px(1.));
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    frame(visual, 0);
    let pan = preview.read_with(visual, |view, _| view.viewport.pan);
    assert!(pan.x > px(0.) && pan.y > px(0.));
    assert!((pan.x - (image.size.width - canvas.size.width) / 2.).abs() < px(1.));
    assert!((pan.y - (image.size.height - canvas.size.height) / 2.).abs() < px(1.));
    visual.simulate_mouse_up(
        point(canvas.right() + px(10.), canvas.bottom() + px(10.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    assert!(preview.read_with(visual, |view, _| view.viewport.drag.is_none()));
}

#[gpui::test]
fn controls_and_original_download(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| cx.set_reduce_motion(false));
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let mut bytes = Vec::new();
        let pixels = RgbaImage::from_pixel(1200, 800, Rgba([220, 30, 60, 128]));
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(&pixels, 1200, 800, image::ExtendedColorType::Rgba8)
            .unwrap();
        let paths: Vec<_> = ["first.png", "second.png"]
            .into_iter()
            .map(|name| {
                let path = fixture.directory.path().join(name);
                std::fs::write(&path, &bytes).unwrap();
                path
            })
            .collect();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.add_images(paths, window, cx));
        });
        wait(visual, |cx| view.read(cx).image_ready());
        let previous_focus = visual.update(|window, cx| {
            view.update(cx, |view, cx| view.focus(window, cx));
            window.focused(cx).unwrap()
        });
        let preview = visual.update(|window, cx| {
            let view = view.read(cx);
            let sources = view
                .image_attachments()
                .into_iter()
                .map(ImageSource::from)
                .collect();
            let images = view.image_cache();
            open_gallery(&images, sources, 0, window, cx)
        });
        loaded(&preview, visual);
        let entering = visual.debug_bounds("image-lightbox-image").unwrap().size;
        frame(visual, 90);
        let intermediate = visual.debug_bounds("image-lightbox-image").unwrap().size;
        frame(visual, 500);
        let opened = visual.debug_bounds("image-lightbox-image").unwrap().size;
        assert!(entering.width < intermediate.width && intermediate.width < opened.width);
        let handle = visual.update(|window, _| window.window_handle());
        for viewport in [size(px(1280.), px(820.)), size(px(640.), px(480.))] {
            visual.simulate_window_resize(handle, viewport);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let overlay = visual.debug_bounds("attachment-image-preview").unwrap();
            let canvas = visual.debug_bounds("image-lightbox-canvas").unwrap();
            let toolbar = visual.debug_bounds("image-lightbox-toolbar").unwrap();
            assert_eq!(overlay.origin, point(px(0.), px(0.)));
            assert_eq!(overlay.size, viewport);
            assert_eq!(canvas.center().x, overlay.center().x);
            assert_eq!(toolbar.center().x, overlay.center().x);
            assert!(toolbar.top() >= canvas.bottom());
            assert!(toolbar.bottom() < overlay.bottom());
            assert!(canvas.size.height > px(200.));
            let image = visual.debug_bounds("image-lightbox-image").unwrap();
            assert!(image.size.width <= px(960.) && image.size.height <= px(640.));
            assert!(image.size.width <= canvas.size.width * 0.82 + px(1.));
            assert!(image.size.height <= canvas.size.height * 0.82 + px(1.));
        }
        for _ in 0..12 {
            visual.simulate_keystrokes("tab");
            assert!(
                visual.update(|window, cx| { preview.read(cx).focus.contains_focused(window, cx) })
            );
        }
        let fitted = visual.debug_bounds("image-lightbox-image").unwrap().size;
        tap(visual, "image-zoom-in");
        assert_eq!(
            preview.read_with(visual, |view, _| view.viewport.zoom),
            1.25
        );
        frame(visual, 50);
        let growing = visual.debug_bounds("image-lightbox-image").unwrap().size;
        assert!(growing.width > fitted.width && growing.width < fitted.width * 1.25);
        frame(visual, 500);
        assert!(
            (visual
                .debug_bounds("image-lightbox-image")
                .unwrap()
                .size
                .width
                - fitted.width * 1.25)
                .abs()
                < px(1.)
        );
        tap(visual, "image-zoom-out");
        frame(visual, 500);
        assert_eq!(preview.read_with(visual, |view, _| view.viewport.zoom), 1.);
        let position = visual
            .debug_bounds("image-lightbox-canvas")
            .unwrap()
            .center();
        visual.simulate_mouse_move(position, None, Modifiers::default());
        visual.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Lines(point(0., 2.)),
            ..Default::default()
        });
        let wheeled = preview.read_with(visual, |view, _| view.viewport.zoom);
        assert!(wheeled > 1.);
        visual.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-24.))),
            ..Default::default()
        });
        let zoom = preview.read_with(visual, |view, _| view.viewport.zoom);
        assert!(zoom < wheeled && zoom > 1.);
        pinch(visual, 0.25);
        assert_eq!(
            preview.read_with(visual, |view, _| view.viewport.zoom),
            zoom * 1.25
        );
        pinch(visual, -0.2);
        assert!((preview.read_with(visual, |view, _| view.viewport.zoom) - zoom).abs() < 0.001);
        tap(visual, "image-fit");
        assert_eq!(
            preview.read_with(visual, |view, _| view.viewport.clone()),
            Viewport::default()
        );
        frame(visual, 500);
        pinch(visual, 1.);
        frame(visual, 500);
        drag_to_edge(&preview, visual);
        tap(visual, "image-fit");
        assert_eq!(
            preview.read_with(visual, |view, _| view.viewport.clone()),
            Viewport::default()
        );
        frame(visual, 500);
        tap(visual, "image-rotate");
        loaded(&preview, visual);
        assert_eq!(
            sizes_to_viewport(&preview, visual),
            size(DevicePixels(800), DevicePixels(1200))
        );
        tap(visual, "image-flip-horizontal");
        loaded(&preview, visual);
        tap(visual, "image-flip-vertical");
        loaded(&preview, visual);
        pinch(visual, 1.);
        frame(visual, 500);
        tap(visual, "image-next");
        loaded(&preview, visual);
        assert_eq!(preview.read_with(visual, |view, _| view.index), 1);
        assert_eq!(
            preview.read_with(visual, |view, _| view.viewport.clone()),
            Viewport::default()
        );
        assert_eq!(
            sizes_to_viewport(&preview, visual),
            size(DevicePixels(1200), DevicePixels(800))
        );
        // Navigation keys also work after focus has moved to a toolbar button.
        visual.simulate_keystrokes("left");
        loaded(&preview, visual);
        assert_eq!(preview.read_with(visual, |view, _| view.index), 0);
        tap(visual, "image-rotate");
        loaded(&preview, visual);
        tap(visual, "image-download");
        assert!(visual.did_prompt_for_new_path());
        let destination = fixture.directory.path().join("original.png");
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        fixture::finish_download(visual, &destination);
        assert_eq!(std::fs::read(destination).unwrap(), bytes);
        visual.update(|window, cx| preview.read(cx).focus.clone().focus(window, cx));
        visual.simulate_keystrokes("enter");
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        // Escape must also dismiss while a toolbar control owns focus.
        visual.simulate_keystrokes("tab");
        frame(visual, 500);
        let before_close = visual.debug_bounds("image-lightbox-image").unwrap().size;
        visual.simulate_keystrokes("escape");
        frame(visual, 0);
        assert!(preview.read_with(visual, |view, _| view.closing));
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        frame(visual, 90);
        assert!(
            visual
                .debug_bounds("image-lightbox-image")
                .unwrap()
                .size
                .width
                < before_close.width
        );
        assert!(preview.read_with(visual, |view, _| view.result.is_some()));
        frame(visual, 500);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(preview.read_with(visual, |view, _| view.result.is_none()));
        assert!(visual.update(|window, _| previous_focus.is_focused(window)));

        let sources = preview.read_with(visual, |view, _| view.sources.clone());
        let reduced = visual.update(|window, cx| {
            cx.set_reduce_motion(true);
            let images = view.read(cx).image_cache();
            open_gallery(&images, sources, 0, window, cx)
        });
        loaded(&reduced, visual);
        let fitted = visual.debug_bounds("image-lightbox-image").unwrap().size;
        let canvas = visual.debug_bounds("image-lightbox-canvas").unwrap().size;
        let expected = viewport::fit(sizes_to_viewport(&reduced, visual), canvas);
        assert!((fitted.width - expected.width).abs() < px(1.));
        assert!((fitted.height - expected.height).abs() < px(1.));
        tap(visual, "image-zoom-in");
        frame(visual, 0);
        assert!(
            (visual
                .debug_bounds("image-lightbox-image")
                .unwrap()
                .size
                .width
                - fitted.width * 1.25)
                .abs()
                < px(1.)
        );
        visual.simulate_keystrokes("escape");
        frame(visual, 0);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(reduced.read_with(visual, |view, _| view.result.is_none()));
        assert!(visual.update(|window, _| previous_focus.is_focused(window)));
        visual.update(|_, cx| cx.set_reduce_motion(false));
        visual.update(|window, _| window.remove_window());
        drop(reduced);
        drop(preview);
        drop(view);
        fixture.close();
    }
}
