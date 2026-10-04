//! Dot-matrix loaders; Kit's rotating icon cannot reproduce this geometry.
//! Adapted from Bezel 4a7505ab (MIT), ui/loaders.rs and motion/phase.rs.
//! See third_party_licenses/bezel.md. The compact loader preserves the original proportions.
use gpui_kit::{component::ActiveTheme, *};

const PERIOD: f32 = 0.75;
const DIM: f32 = 0.1;
const RING: [[usize; 2]; 3] = [[0, 1], [5, 2], [4, 3]];

/// A compact 2 × 3 loader centered in the normal, theme-scaled icon slot.
pub(crate) fn mini() -> Div {
    dots(RING, 6.4)
}

/// The activity rail uses a full, three-by-three matrix at navigation icon size.
#[cfg(test)]
pub(crate) fn grid() -> Div {
    dots([[0, 1, 2], [3, 4, 5], [6, 7, 8]], 4.8).debug_selector(|| "activity-loading-grid".into())
}

fn dots<const COLUMNS: usize>(phases: [[usize; COLUMNS]; 3], scale: f32) -> Div {
    div().size_4().flex_shrink_0().child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                let Some(t) = super::motion::seconds(bounds, true, 0., window, cx) else {
                    return;
                };
                let cell = bounds.size.width.min(bounds.size.height) / scale;
                let grid = size(cell * (COLUMNS as f32 * 1.5 - 0.5), cell * 4.);
                let origin = bounds.center() - point(grid.width / 2., grid.height / 2.);
                let colors = [cx.theme().info, cx.theme().warning, cx.theme().danger];
                window.paint_layer(bounds, |window| {
                    for (row, positions) in phases.iter().enumerate() {
                        for (col, position) in positions.iter().enumerate() {
                            let alpha =
                                opacity(t / PERIOD + *position as f32 / (COLUMNS * 3) as f32);
                            let at =
                                origin + point(cell * 1.5 * col as f32, cell * 1.5 * row as f32);
                            window.paint_quad(
                                fill(
                                    Bounds::new(at, size(cell, cell)),
                                    colors[row].opacity(alpha),
                                )
                                .corner_radii(cell / 2.),
                            );
                        }
                    }
                });
            },
        )
        .size_full(),
    )
}

fn opacity(t: f32) -> f32 {
    let t = t.rem_euclid(1.);
    if t < 0.45 {
        1. + (DIM - 1.) * t / 0.45
    } else if t < 0.92 {
        DIM
    } else {
        DIM + (1. - DIM) * (t - 0.92) / 0.08
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn wave_timing() {
        assert_eq!(opacity(0.), 1.);
        assert!((opacity(0.45) - DIM).abs() < 1e-6);
        assert_eq!(opacity(0.9), DIM);
        assert!((opacity(0.96) - 0.55).abs() < 1e-5);
        for step in 0..100 {
            let phase = step as f32 / 100.;
            assert!((opacity(phase) - opacity(phase + 1.)).abs() < 1e-5);
        }
    }

    #[test]
    fn clockwise_chase() {
        let cells = [(0, 0), (0, 1), (1, 1), (2, 1), (2, 0), (1, 0)];
        for (phase, (row, col)) in cells.into_iter().enumerate() {
            assert_eq!(RING[row][col], phase);
        }
    }
}
