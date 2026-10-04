use super::*;
use core::prelude::v1::test;

struct Surface;

impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn fits_content_and_scrolls_overflow(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let surface = cx.new(|_| Surface);
        Root::new(surface, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1400.)));
    let editor = visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        open(owner, None, window, cx)
    });
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(visual);

    let compact = visual.debug_bounds("provider-editor").unwrap();
    let title = visual.debug_bounds("provider-dialog-title").unwrap();
    let save = visual.debug_bounds("provider-save").unwrap();
    assert!(compact.size.height > px(300.));
    assert!(
        save.bottom() - title.top() < px(800.),
        "{compact:?}, {save:?}"
    );
    assert!(editor.read_with(visual, |editor, _| editor.scroll.max_offset().y) < px(1.));

    // Expanded model fields grow the dialog until the available height is used.
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            for _ in 0..5 {
                editor.add_model(window, cx);
            }
        });
    });
    draw(visual);
    let expanded = visual.debug_bounds("provider-editor").unwrap();
    assert!(expanded.size.height > compact.size.height + px(100.));
    assert!(editor.read_with(visual, |editor, _| editor.scroll.max_offset().y) > px(0.));

    // Resizing must keep the title and footer visible while only the body scrolls.
    visual.simulate_window_resize(handle, size(px(760.), px(560.)));
    draw(visual);
    let body = visual.debug_bounds("provider-editor").unwrap();
    let title = visual.debug_bounds("provider-dialog-title").unwrap();
    let save = visual.debug_bounds("provider-save").unwrap();
    assert!(title.top() >= px(0.));
    assert!(body.top() >= title.bottom());
    assert!(body.bottom() <= save.top());
    assert!(save.bottom() < px(560.));
    let before = editor.read_with(visual, |editor, _| editor.scroll.offset());
    visual.simulate_event(ScrollWheelEvent {
        position: point(body.right() - px(12.), body.bottom() - px(12.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    draw(visual);
    assert!(editor.read_with(visual, |editor, _| editor.scroll.offset().y) < before.y);
    assert_eq!(visual.debug_bounds("provider-save").unwrap(), save);
    assert_eq!(visual.debug_bounds("provider-dialog-title").unwrap(), title);

    // Removing content restores the compact height and clears obsolete scroll space.
    visual.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            editor.models.clear();
            cx.notify();
        });
    });
    visual.simulate_window_resize(handle, size(px(1280.), px(1400.)));
    draw(visual);
    let restored = visual.debug_bounds("provider-editor").unwrap();
    assert!((restored.size.height - compact.size.height).abs() < px(1.));
    assert!(editor.read_with(visual, |editor, _| editor.scroll.max_offset().y) < px(1.));
    let cancel = visual.debug_bounds("provider-cancel").unwrap();
    visual.simulate_click(cancel.center(), Modifiers::default());
    draw(visual);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}
