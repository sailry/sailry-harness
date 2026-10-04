use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, Theme, ThemeMode};

struct Track {
    state: Entity<SliderState>,
    focus: FocusHandle,
}

impl Render for Track {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(280.))
            .child(super::render(&self.state, &self.focus, 3, false, false, cx))
    }
}

#[gpui::test]
fn fill_and_thumb_share_scaled_geometry(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let slider = cx.new(|_| SliderState::new().min(0.).max(2.).step(1.));
        state = Some(slider.clone());
        let track = cx.new(|cx| Track {
            state: slider,
            focus: cx.focus_handle(),
        });
        Root::new(track, window, cx)
    });
    let state = state.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for rem in [14., 16., 20.] {
            for value in [0., 1., 2.] {
                visual.update(|window, cx| {
                    Theme::change(mode, Some(window), cx);
                    Theme::update(cx, |theme| theme.font_size = px(rem));
                    state.update(cx, |state, cx| state.set_value(value, window, cx));
                    window.refresh();
                });
                visual.run_until_parked();
                visual.update(|window, cx| window.draw(cx).clear(cx));
                let track = visual.debug_bounds("model-effort-track").unwrap();
                let thumb = visual.debug_bounds("model-effort-thumb").unwrap();
                let fill = visual.debug_bounds("model-effort-fill");
                assert!(thumb.left() >= track.left() && thumb.right() <= track.right());
                assert_eq!(thumb.size.width, px(rem * 2.));
                if value == 0. {
                    assert_eq!(thumb.left(), track.left());
                    assert!(fill.is_none());
                } else {
                    let fill = fill.unwrap();
                    assert!((fill.right() - thumb.center().x).abs() < px(0.1));
                    assert!((thumb.center().y - fill.center().y).abs() < px(0.1));
                }
                if value == 2. {
                    assert_eq!(thumb.right(), track.right());
                }
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}
