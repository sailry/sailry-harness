//! Elliptical status rings for activity scenes. Script elements only offer
//! one-shot transitions of opacity and box geometry, so the looping highlight
//! that travels around a running ring is painted by this native canvas. It
//! shares the bounded redraw clock, so clipped or inactive rings stay idle.
use gpui_kit::*;

/// Seconds for the running highlight to travel once around the ring.
const PERIOD: f32 = 1.6;
/// Fraction of the circumference covered by the running highlight.
const SEGMENT: f32 = 0.24;
/// Track opacity under the running highlight.
const TRACK: f32 = 0.35;
const STEPS: f32 = 64.;

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

    #[test]
    fn rejects_degenerate_rings() {
        let flat = Bounds::new(point(px(0.), px(0.)), size(px(40.), px(1.)));
        assert!(arc(flat, px(1.5), 0., 1.).is_none());
        assert!(arc(bounds(), px(1.5), 0., 0.).is_none());
    }
}
