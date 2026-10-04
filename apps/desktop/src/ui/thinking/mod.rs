//! Bezel thinking orbs on Kit's retained state and theme.
//! Adapted from Bezel 4a7505ab and gpui-thinking-orbs (MIT).
//! See third_party_licenses/bezel.md. Kit has no equivalent particle indicator.
use gpui_kit::{component::ActiveTheme, *};
use std::{cell::RefCell, rc::Rc};

mod engine;
mod paint;
mod presets;
mod types;
use engine::Frame;
use presets::resolve_preset;
use types::{OrbSize, OrbState};

/// Uses the phase already determined by the conversation projection.
pub(crate) fn render(id: impl Into<SharedString>, phase: &str, size: Pixels) -> AnyElement {
    let state = state(phase);
    Orb {
        id: id.into(),
        state: state.unwrap_or(OrbState::Breathing),
        size,
        animated: state.is_some(),
    }
    .into_any_element()
}

fn state(phase: &str) -> Option<OrbState> {
    match phase {
        // Use Bezel's dotted scanning globe; the caller keeps the thinking label.
        "turn_awaiting_response" | "turn_thinking" => Some(OrbState::Searching),
        "turn_generating" => Some(OrbState::Composing),
        "turn_tools_running" => Some(OrbState::Working),
        "turn_waiting" => Some(OrbState::Breathing),
        _ => None,
    }
}

#[derive(Default)]
struct Geometry {
    key: Option<(OrbState, f32, f32)>,
    frame: Frame,
}

#[derive(IntoElement)]
struct Orb {
    id: SharedString,
    state: OrbState,
    size: Pixels,
    animated: bool,
}

impl RenderOnce for Orb {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let geometry = window
            .use_keyed_state(format!("{}-orb-geometry", self.id), cx, |_, _| {
                Rc::new(RefCell::new(Geometry::default()))
            })
            .read(cx)
            .clone();
        let preset = resolve_preset(
            self.state,
            if self.size <= px(32.) {
                OrbSize::Inline
            } else if self.size <= px(80.) {
                OrbSize::Avatar
            } else if self.size <= px(112.) {
                OrbSize::Large
            } else {
                OrbSize::Hero
            },
        );
        let selector = self.id.clone();
        div()
            .id(self.id)
            .debug_selector(move || selector.to_string())
            .size(self.size)
            .flex_shrink_0()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        let Some(t) =
                            super::motion::seconds(bounds, self.animated, 0.6, window, cx)
                        else {
                            return;
                        };
                        let t = if cx.reduce_motion() || !self.animated {
                            0.6
                        } else {
                            t * preset.speed
                        };
                        let edge = bounds.size.width.min(bounds.size.height);
                        let edge_px = f32::from(edge);
                        let key = (self.state, edge_px, t);
                        let mut geometry = geometry.borrow_mut();
                        if geometry.key != Some(key) {
                            engine::draw_mode_into_resolved(
                                preset.mode,
                                edge_px,
                                t,
                                &preset.opts,
                                &mut geometry.frame,
                            );
                            geometry.key = Some(key);
                        }
                        let bounds = Bounds::new(
                            bounds.center() - point(edge / 2., edge / 2.),
                            size(edge, edge),
                        );
                        let ink = paint::Ink {
                            foreground: cx.theme().foreground,
                            background: cx.theme().background,
                        };
                        paint::paint_frame(
                            window,
                            bounds,
                            &geometry.frame,
                            ink,
                            preset.opts.r_min.unwrap_or(0.3),
                        );
                    },
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests;
