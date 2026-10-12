use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, button::Button};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Region {
    clicks: Rc<Cell<usize>>,
}

impl Render for Region {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .relative()
            .size_full()
            .child(
                Button::new("covered")
                    .debug_selector(|| "covered".into())
                    .label("Covered")
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
            )
            .child(overlay("region-overlay", cx))
    }
}

fn mount(cx: &mut TestAppContext) -> (&mut VisualTestContext, Rc<Cell<usize>>) {
    crate::conversation::live::tests::fixture::init(cx);
    LAST_OPACITY.with(|last| last.set(None));
    let clicks = Rc::new(Cell::new(0));
    let owner = clicks.clone();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let region = cx.new(|_| Region { clicks: owner });
        Root::new(region, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (visual, clicks)
}

fn opacity() -> f32 {
    LAST_OPACITY.with(Cell::get).expect("overlay was drawn")
}

fn redraw(visual: &mut VisualTestContext) {
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn starts_transparent(cx: &mut TestAppContext) {
    let (visual, _) = mount(cx);
    assert!(visual.debug_bounds("region-overlay").is_some());
    assert_eq!(opacity(), 0.);
}

#[gpui::test]
fn becomes_opaque_after_delay(cx: &mut TestAppContext) {
    let (visual, _) = mount(cx);
    // Element animations read the wall clock, not the test executor clock.
    std::thread::sleep(FADE + Duration::from_millis(50));
    redraw(visual);
    assert_eq!(opacity(), 1.);
}

#[gpui::test]
fn reduced_motion_shows_immediately(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let _ = mount(cx);
    assert_eq!(opacity(), 1.);
}

#[gpui::test]
fn blocks_covered_content(cx: &mut TestAppContext) {
    let (visual, clicks) = mount(cx);
    let center = visual.debug_bounds("covered").unwrap().center();
    visual.simulate_mouse_move(center, None, Modifiers::default());
    redraw(visual);
    visual.simulate_click(center, Modifiers::default());
    visual.run_until_parked();
    assert_eq!(clicks.get(), 0);
}

#[test]
fn fade_starts_after_half() {
    assert_eq!(fade(0.), 0.);
    assert_eq!(fade(0.5), 0.);
    assert_eq!(fade(0.75), 0.5);
    assert_eq!(fade(1.), 1.);
}
