//! The outer material occupies only the area outside the content's rounded edge.
use super::*;

// Kit's WindowBorder documents rectangular child masks. A rounded Div paints
// its inside only; painting a rectangular underlay would compound container alpha.
// This decorative path fills the complementary corner without adding any hitbox.
pub(super) fn material(radius: Pixels, color: Hsla) -> impl IntoElement {
    div()
        .debug_selector(|| "shell-corner-material".into())
        .absolute()
        .left(px(RAIL_WIDTH))
        .top(px(HEADER_HEIGHT))
        .size(radius)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if let Some(path) = outside(bounds.origin, radius) {
                        window.paint_path(path, color);
                    }
                },
            )
            .size_full(),
        )
}

fn outside(origin: Point<Pixels>, radius: Pixels) -> Option<Path<Pixels>> {
    if radius <= px(0.) {
        return None;
    }
    let mut path = PathBuilder::fill();
    path.move_to(origin);
    path.line_to(origin + point(radius, px(0.)));
    path.arc_to(
        point(radius, radius),
        px(0.),
        false,
        false,
        origin + point(px(0.), radius),
    );
    path.close();
    Some(path.build().expect("corner material path"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn fills_only_the_exterior() {
        for radius in [8., 16., 32.] {
            let origin = point(px(54.), px(48.));
            let path = outside(origin, px(radius)).unwrap();
            let area: f32 = path
                .vertices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| {
                    let a = triangle[0].xy_position;
                    let b = triangle[1].xy_position;
                    let c = triangle[2].xy_position;
                    ((f32::from(b.x - a.x) * f32::from(c.y - a.y)
                        - f32::from(b.y - a.y) * f32::from(c.x - a.x))
                        / 2.)
                        .abs()
                })
                .sum();
            let expected = radius * radius * (1. - std::f32::consts::PI / 4.);
            assert!(
                (area - expected).abs() < radius * 0.2,
                "{area} != {expected}"
            );
            for vertex in &path.vertices {
                let p = vertex.xy_position - origin;
                // Arc tessellation can round an endpoint by a few float ULPs.
                let epsilon = px(0.0001);
                assert!(
                    p.x >= -epsilon
                        && p.y >= -epsilon
                        && p.x <= px(radius) + epsilon
                        && p.y <= px(radius) + epsilon,
                    "vertex {p:?}, radius {radius}"
                );
            }
        }
        assert!(outside(point(px(0.), px(0.)), px(0.)).is_none());
    }
}
