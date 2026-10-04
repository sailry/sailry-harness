//! Original square-dot artwork with pointer-driven illumination.
//! Kit b79f4ce's Icon supplies one monochrome tint, not spatial lighting.
//! Keep its stable SVG as the dim base and use Kit's GPUI canvas only for
//! illuminated particles and glow, with the same semantic foreground.
use super::*;

const SVG: &[u8] = include_bytes!("wordmark.svg");
const WIDTH: f32 = 328.;
const HEIGHT: f32 = 112.;
pub(super) const RATIO: f32 = WIDTH / HEIGHT;

#[derive(Default)]
pub(super) struct Light {
    pub(super) pointer: Option<Pixels>,
}

impl Light {
    pub(super) fn set(&mut self, pointer: Pixels) -> bool {
        if self.pointer == Some(pointer) {
            return false;
        }
        self.pointer = Some(pointer);
        true
    }
}

pub(super) fn render(height: Pixels, pointer: Option<Pixels>, cx: &App) -> impl IntoElement {
    let ink = cx.theme().foreground;
    div()
        .id("welcome-dots")
        .debug_selector(|| "welcome-dots".into())
        .role(Role::Image)
        .aria_label(tr("welcome_name"))
        .w(height * RATIO)
        .h(height)
        .flex_shrink_0()
        .relative()
        .child(
            Icon::default()
                .data(SVG)
                .w_full()
                .h_full()
                .opacity(0.18)
                .text_color(ink),
        )
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    if bounds.size.width <= px(0.) || bounds.size.height <= px(0.) {
                        return;
                    }
                    let center = beam(bounds, pointer);
                    let scale = bounds.size.height / HEIGHT;
                    // Paint all light spill before the square cores to preserve
                    // the original particle edges under overlapping halos.
                    for &(x, y) in particles() {
                        let strength = strength(x + 3., center);
                        if strength < 0.005 {
                            continue;
                        }
                        let dot = particle(bounds, x, y);
                        window.paint_drop_shadows(
                            dot,
                            Corners::default(),
                            &[
                                BoxShadow::new(px(0.), px(0.), ink.opacity(strength * 0.26))
                                    .blur_radius(scale * 5.),
                                BoxShadow::new(px(0.), px(0.), ink.opacity(strength * 0.09))
                                    .blur_radius(scale * 13.),
                            ],
                        );
                    }
                    for &(x, y) in particles() {
                        let strength = strength(x + 3., center);
                        window
                            .paint_quad(fill(particle(bounds, x, y), ink.opacity(strength * 0.86)));
                    }
                },
            )
            .absolute()
            .inset_0()
            .size_full(),
        )
}

fn beam(bounds: Bounds<Pixels>, pointer: Option<Pixels>) -> f32 {
    pointer
        .filter(|_| bounds.size.width > px(0.))
        .map(|x| ((x - bounds.left()) / bounds.size.width).clamp(0., 1.))
        .unwrap_or(0.5)
        * WIDTH
}

fn strength(x: f32, center: f32) -> f32 {
    let distance = (x - center) / (WIDTH * 0.13);
    (-0.5 * distance * distance).exp()
}

fn particle(bounds: Bounds<Pixels>, x: f32, y: f32) -> Bounds<Pixels> {
    let scale_x = bounds.size.width / WIDTH;
    let scale_y = bounds.size.height / HEIGHT;
    Bounds::new(
        bounds.origin + point(scale_x * x, scale_y * y),
        size(scale_x * 6., scale_y * 6.),
    )
}

fn particles() -> &'static [(f32, f32)] {
    static PARTICLES: std::sync::LazyLock<Vec<(f32, f32)>> = std::sync::LazyLock::new(|| {
        // This is the embedded square-path artwork, not a general SVG loader.
        include_str!("wordmark.svg")
            .split("<path ")
            .skip(1)
            .flat_map(|path| {
                path.split_once(" d=\"")
                    .and_then(|(_, path)| path.split_once('"'))
                    .expect("wordmark paths must have square-particle coordinates")
                    .0
                    .split('M')
                    .skip(1)
            })
            .map(|dot| {
                let (x, y) = dot
                    .strip_suffix("h6v6h-6z")
                    .and_then(|dot| dot.split_once(' '))
                    .expect("wordmark particles must be square");
                (x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    });
    &PARTICLES
}

#[cfg(test)]
mod tests;
