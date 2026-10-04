use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, list::ListItem};
use std::time::Duration;

struct Harness {
    hovered: bool,
    text: SharedString,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ListItem::new("row")
            .debug_selector(|| "row".into())
            .w(px(160.))
            .h(px(32.))
            .child(Title::new("title".into(), self.text.clone(), self.hovered))
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
    }
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<Harness>, &'a mut VisualTestContext) {
    cx.update(gpui_kit::init);
    let text: SharedString = text.to_owned().into();
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            hovered: false,
            text,
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (entity.unwrap(), visual)
}

fn advance(visual: &mut VisualTestContext, delay: Duration) {
    visual.executor().advance_clock(delay);
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn long_title_scrolls_only_while_hovered(cx: &mut TestAppContext) {
    let (_, visual) = mount(
        cx,
        "Inspect the workspace and summarize all pending changes 中文 🙂",
    );
    let row = visual.debug_bounds("row").unwrap();
    let resting = visual.debug_bounds("title").unwrap();
    visual.simulate_mouse_move(row.center(), None, Modifiers::default());
    advance(visual, Duration::from_millis(300));
    let initial = visual.debug_bounds("title-text").unwrap();
    assert_eq!(initial.left(), resting.left());
    assert!(initial.size.width > resting.size.width);
    advance(visual, Duration::from_secs(2));
    let moving = visual.debug_bounds("title-text").unwrap();
    assert!(moving.left() < initial.left());
    assert_eq!(visual.debug_bounds("row").unwrap(), row);
    assert_eq!(visual.debug_bounds("title").unwrap(), resting);
    advance(visual, Duration::from_secs(60));
    let end = visual.debug_bounds("title-text").unwrap();
    assert!((end.right() - resting.right()).abs() < px(1.));
    advance(visual, Duration::from_secs(60));
    assert_eq!(visual.debug_bounds("title-text").unwrap(), end);
    visual.simulate_mouse_move(point(px(300.), px(100.)), None, Modifiers::default());
    advance(visual, Duration::from_millis(100));
    assert!(visual.debug_bounds("title-text").is_none());
    assert_eq!(visual.debug_bounds("title").unwrap(), resting);
    visual.simulate_mouse_move(row.center(), None, Modifiers::default());
    advance(visual, Duration::from_millis(100));
    assert_eq!(
        visual.debug_bounds("title-text").unwrap().left(),
        initial.left()
    );
}

#[gpui::test]
fn short_titles_and_reduced_motion_stay_still(cx: &mut TestAppContext) {
    let (view, visual) = mount(cx, "Short");
    let row = visual.debug_bounds("row").unwrap();
    visual.simulate_mouse_move(row.center(), None, Modifiers::default());
    advance(visual, Duration::from_millis(100));
    let initial = visual.debug_bounds("title-text").unwrap();
    advance(visual, Duration::from_secs(10));
    assert_eq!(visual.debug_bounds("title-text").unwrap(), initial);
    visual.update(|_, cx| {
        cx.set_reduce_motion(true);
        view.update(cx, |view, cx| {
            view.text = "A long title which would otherwise scroll beyond its viewport".into();
            cx.notify();
        });
    });
    advance(visual, Duration::from_secs(10));
    assert!(visual.debug_bounds("title-text").is_none());
    assert_eq!(visual.debug_bounds("row").unwrap(), row);
}
