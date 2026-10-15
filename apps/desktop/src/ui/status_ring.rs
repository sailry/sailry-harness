//! Elliptical status rings for activity scenes. Script elements only offer
//! one-shot transitions of opacity and box geometry, so the looping highlight
//! that travels around a running ring is painted by this native canvas. It
//! shares the bounded redraw clock, so clipped or inactive rings stay idle.
//! Progress rings reuse the same stroked geometry. Kit's `ProgressCircle`
//! fills a thick annulus whose full track is one contour closed by a
//! sub-pixel seam; in the activity rail it rendered as a filled half-disc.
use gpui_kit::*;

/// Seconds for the running highlight to travel once around the ring.
const PERIOD: f32 = 1.6;
/// Fraction of the circumference covered by the running highlight.
const SEGMENT: f32 = 0.24;
/// Track opacity under the running highlight.
const TRACK: f32 = 0.35;
const STEPS: f32 = 64.;
/// Start offset, in turns, that places an arc's origin at 12 o'clock.
const TOP: f32 = -0.25;

pub(crate) fn render(id: String, color: Hsla, line: Pixels, running: bool) -> AnyElement {
    div()
        .debug_selector(move || id.clone())
        .size_full()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, cx| {
                    let Some(seconds) = super::motion::seconds(bounds, running, 0., window, cx)
                    else {
                        return;
                    };
                    let track = if running { color.opacity(TRACK) } else { color };
                    if let Some(path) = arc(bounds, line, 0., 1.) {
                        window.paint_path(path, track);
                    }
                    if running
                        && let Some(path) = arc(bounds, line, (seconds / PERIOD).fract(), SEGMENT)
                    {
                        window.paint_path(path, color);
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// A progress ring: a faint track plus a clockwise arc from 12 o'clock that
/// covers `fraction` of a turn. Unknown progress shows the running highlight.
pub(crate) fn progress(id: String, color: Hsla, line: Pixels, fraction: Option<f32>) -> AnyElement {
    let Some(fraction) = fraction else {
        return render(id, color, line, true);
    };
    div()
        .debug_selector(move || id.clone())
        .size_full()
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    for (path, color) in measured(bounds, line, fraction, color) {
                        window.paint_path(path, color);
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// The track and value arcs of a determinate progress ring.
fn measured(
    bounds: Bounds<Pixels>,
    line: Pixels,
    fraction: f32,
    color: Hsla,
) -> impl Iterator<Item = (Path<Pixels>, Hsla)> {
    [
        (arc(bounds, line, 0., 1.), color.opacity(TRACK)),
        (arc(bounds, line, TOP, fraction.clamp(0., 1.)), color),
    ]
    .into_iter()
    .filter_map(|(path, color)| Some((path?, color)))
}

/// Strokes `length` turns of the ellipse inscribed in `bounds`, starting at
/// `start` turns clockwise from the right-hand end of its horizontal axis.
fn arc(bounds: Bounds<Pixels>, line: Pixels, start: f32, length: f32) -> Option<Path<Pixels>> {
    let radius = size(
        (bounds.size.width - line) / 2.,
        (bounds.size.height - line) / 2.,
    );
    if radius.width <= px(0.) || radius.height <= px(0.) || length <= 0. {
        return None;
    }
    let center = bounds.center();
    let steps = (STEPS * length.min(1.)).ceil().max(2.) as usize;
    let mut path = PathBuilder::stroke(line);
    for step in 0..=steps {
        let angle = (start + length * step as f32 / steps as f32) * std::f32::consts::TAU;
        let at = center + point(radius.width * angle.cos(), radius.height * angle.sin());
        if step == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    if length >= 1. {
        path.close();
    }
    path.build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(10.), px(20.)), size(px(60.), px(14.)))
    }

    #[test]
    fn stays_inside_bounds() {
        let line = px(1.5);
        for (start, length) in [(0., 1.), (0.9, SEGMENT), (0.5, SEGMENT)] {
            let path = arc(bounds(), line, start, length).unwrap();
            let outer = bounds().dilate(px(0.1));
            for vertex in &path.vertices {
                assert!(outer.contains(&vertex.xy_position), "{vertex:?}");
            }
        }
    }

    fn square() -> Bounds<Pixels> {
        Bounds::new(point(px(8.), px(700.)), size(px(32.), px(32.)))
    }

    #[test]
    fn progress_starts_at_top() {
        let line = px(2.);
        let rings: Vec<_> = measured(square(), line, 0.07, Hsla::red()).collect();
        assert_eq!(rings.len(), 2);
        let center = square().center();
        let outer = square().dilate(px(0.1));
        for vertex in &rings[1].0.vertices {
            let at = vertex.xy_position;
            assert!(outer.contains(&at), "{vertex:?}");
            // 7% sweeps about 25 degrees clockwise from 12 o'clock, so the
            // stroke stays in the top of the ring, right of its vertical axis.
            assert!(at.y < center.y - px(10.), "{vertex:?}");
            assert!(at.x > center.x - line, "{vertex:?}");
        }
        // Stroked arcs leave the ring's interior uncovered.
        let hole = Bounds::centered_at(center, size(px(16.), px(16.)));
        for (path, _) in &rings {
            for vertex in &path.vertices {
                assert!(!hole.contains(&vertex.xy_position), "{vertex:?}");
            }
        }
    }

    #[test]
    fn empty_progress_shows_track() {
        assert_eq!(measured(square(), px(2.), 0., Hsla::red()).count(), 1);
    }

    #[test]
    fn rejects_degenerate_rings() {
        let flat = Bounds::new(point(px(0.), px(0.)), size(px(40.), px(1.)));
        assert!(arc(flat, px(1.5), 0., 1.).is_none());
        assert!(arc(bounds(), px(1.5), 0., 0.).is_none());
    }
}
