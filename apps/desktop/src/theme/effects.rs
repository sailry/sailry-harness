//! Kit supplies timed redraws, but no theme particle layer. This inert canvas
//! paints only decoration; controls, focus and input remain framework-owned.
mod motion;

use super::package::{Effect, Kind};
use gpui_kit::*;
use std::{f32::consts::TAU, time::Duration};

pub(crate) fn enabled(cx: &App) -> bool {
    cx.try_global::<crate::preferences::Preferences>()
        .is_none_or(|preferences| preferences.data.theme_effects.unwrap_or(true))
        && !cx.reduce_motion()
}

pub(crate) fn layer(slot: &'static str, cx: &App) -> Option<AnyElement> {
    if !enabled(cx) {
        return None;
    }
    let variant = super::variant(cx)?;
    let effects = variant.effects.get(slot)?;
    if effects.iter().all(|effect| effect.opacity == 0.) {
        return None;
    }
    let catalog = cx.global::<super::Catalog>();
    Some(
        Layer {
            slot,
            identity: format!(
                "{}-{}-{}",
                catalog.packages[catalog.selected].id,
                if variant.theme.mode.is_dark() {
                    "dark"
                } else {
                    "light"
                },
                slot
            ),
            effects: effects.clone(),
        }
        .into_any_element(),
    )
}

#[derive(IntoElement)]
struct Layer {
    slot: &'static str,
    identity: String,
    effects: Vec<Effect>,
}

impl RenderOnce for Layer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let animate = enabled(cx) && window.is_window_active();
        div()
            .absolute()
            .inset_0()
            .size_full()
            .overflow_hidden()
            .debug_selector(move || format!("theme-effects-{}", self.slot))
            .children(
                self.effects
                    .into_iter()
                    .enumerate()
                    .filter(|(_, effect)| effect.opacity > 0.)
                    .map(|(index, effect)| {
                        let duration = effect.duration;
                        let particles = Particles { effect, phase: 0. };
                        if animate {
                            particles
                                .with_animation(
                                    SharedString::from(format!("{}-{index}", self.identity)),
                                    Animation::new(Duration::from_secs_f32(duration))
                                        .repeat_synced()
                                        .with_max_fps(20.),
                                    |mut particles, phase| {
                                        particles.phase = phase;
                                        particles
                                    },
                                )
                                .into_any_element()
                        } else {
                            particles.into_any_element()
                        }
                    }),
            )
    }
}

#[derive(IntoElement)]
struct Particles {
    effect: Effect,
    phase: f32,
}

impl RenderOnce for Particles {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let color = gpui_kit::component::theme::try_parse_color(&self.effect.color)
            .expect("validated theme effect color");
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
                    return;
                }
                window.paint_layer(bounds, |window| {
                    for index in 0..u32::from(self.effect.count) {
                        let sample = motion::sample(index, self.effect.seed, self.phase);
                        let opacity = self.effect.opacity;
                        let extent = self.effect.size * sample.scale;
                        let (x, y, alpha) = match self.effect.kind {
                            Kind::Twinkle => (sample.x, sample.y, opacity * sample.pulse),
                            Kind::Embers => (
                                sample.x + sample.sway,
                                1. - sample.time,
                                opacity * sample.fade,
                            ),
                            Kind::Dust => (
                                sample.x + sample.sway,
                                sample.time,
                                opacity * sample.fade * 0.65,
                            ),
                            Kind::Petals => (
                                sample.x + sample.sway * 3.,
                                sample.time,
                                opacity * sample.fade,
                            ),
                            Kind::Pulse => {
                                (0.5, 0.48, opacity * (0.5 + 0.5 * (self.phase * TAU).sin()))
                            }
                            Kind::Scan => (0.5, sample.time, opacity * sample.fade),
                        };
                        let center =
                            bounds.origin + point(bounds.size.width * x, bounds.size.height * y);
                        let dot = Bounds::new(
                            center - point(px(extent / 2.), px(extent / 2.)),
                            size(px(extent), px(extent)),
                        );
                        let ink = color.opacity(alpha);
                        match self.effect.kind {
                            Kind::Pulse => {
                                let halo = Bounds::new(
                                    center
                                        - point(
                                            bounds.size.width * 0.12,
                                            bounds.size.height * 0.10,
                                        ),
                                    size(bounds.size.width * 0.24, bounds.size.height * 0.20),
                                );
                                window.paint_drop_shadows(
                                    halo,
                                    Corners::all(halo.size.height / 2.),
                                    &[BoxShadow::new(px(0.), px(0.), ink)
                                        .blur_radius(bounds.size.height * 0.25)],
                                );
                            }
                            Kind::Scan => window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.left(), center.y),
                                    size(bounds.size.width, px(extent)),
                                ),
                                ink,
                            )),
                            Kind::Petals => {
                                let mut path = PathBuilder::fill();
                                path.move_to(center - point(px(extent), px(extent * 0.6)));
                                path.curve_to(
                                    center + point(px(extent), px(extent * 0.6)),
                                    center + point(px(extent), px(-extent)),
                                );
                                path.curve_to(
                                    center - point(px(extent), px(extent * 0.6)),
                                    center + point(px(-extent), px(extent)),
                                );
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, ink);
                                }
                            }
                            Kind::Twinkle => {
                                window.paint_quad(
                                    fill(dot, ink).corner_radii(Corners::all(px(extent / 2.))),
                                );
                                window.paint_quad(fill(
                                    Bounds::new(
                                        center - point(px(extent), px(0.35)),
                                        size(px(extent * 2.), px(0.7)),
                                    ),
                                    ink.opacity(0.6),
                                ));
                                window.paint_quad(fill(
                                    Bounds::new(
                                        center - point(px(0.35), px(extent)),
                                        size(px(0.7), px(extent * 2.)),
                                    ),
                                    ink.opacity(0.6),
                                ));
                            }
                            _ => window.paint_quad(
                                fill(dot, ink).corner_radii(Corners::all(px(extent / 2.))),
                            ),
                        }
                    }
                });
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }
}
