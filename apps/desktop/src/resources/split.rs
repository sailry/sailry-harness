//! Keep the resource list fixed while the preview takes the remaining space.
//! Kit retains both measured panel sizes after dragging. Reset its geometry on
//! container resize so it starts from the saved list width instead of scaling it.
use gpui_kit::{component::*, *};

pub(crate) const MIN_WIDTH: f32 = 180.;

struct Geometry {
    state: Entity<ResizableState>,
    width: Pixels,
    controls: Pixels,
}

#[derive(IntoElement)]
pub(crate) struct Split {
    pub id: SharedString,
    pub width: Pixels,
    pub content: AnyElement,
    pub controls: AnyElement,
    pub controls_width: Option<Entity<Pixels>>,
}

impl RenderOnce for Split {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let geometry = window.use_keyed_state(self.id.clone(), cx, |_, cx| Geometry {
            state: cx.new(|_| ResizableState::default()),
            width: px(0.),
            controls: px(220.),
        });
        if geometry.read(cx).width != self.width {
            geometry.update(cx, |geometry, cx| {
                geometry.width = self.width;
                geometry.state.update(cx, |state, _| state.clear());
            });
        }
        let state = geometry.read(cx).state.clone();
        let geometry_state = geometry.read(cx);
        let controls = self
            .controls_width
            .as_ref()
            .map_or(geometry_state.controls, |width| *width.read(cx))
            .min((geometry_state.width - px(MIN_WIDTH)).max(px(MIN_WIDTH)));
        div().size_full().min_w_0().child(
            h_resizable(self.id)
                .with_state(&state)
                .on_resize({
                    let geometry = geometry.clone();
                    let controls_width = self.controls_width.clone();
                    move |state, _, cx| {
                        let width = state.read(cx).sizes().get(1).copied();
                        if let Some(width) = width {
                            if let Some(saved) = &controls_width {
                                saved.update(cx, |saved, cx| {
                                    *saved = width;
                                    cx.notify();
                                });
                            }
                            geometry.update(cx, |geometry, cx| {
                                geometry.controls = width;
                                cx.notify();
                            });
                        }
                    }
                })
                .child(
                    resizable_panel()
                        .size_range(px(MIN_WIDTH)..px(10000.))
                        .child(self.content),
                )
                .child(
                    resizable_panel()
                        .size(controls)
                        .size_range(px(MIN_WIDTH)..px(480.))
                        .flex_none()
                        .child(self.controls),
                ),
        )
    }
}
