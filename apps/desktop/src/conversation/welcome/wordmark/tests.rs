use super::*;
use core::prelude::v1::test;

#[test]
fn artwork_and_geometry() {
    for (scenario, check) in [
        (
            "embedded_svg_is_valid",
            (|_scenario: &str| {
                let renderer = SvgRenderer::new(std::sync::Arc::new(()));
                renderer.parse_svg(SVG).unwrap();
                let source = std::str::from_utf8(SVG).unwrap();
                assert!(
                    source.contains("viewBox=\"0 0 328 112\""),
                    "embedded svg is valid"
                );
                assert_eq!(
                    source.matches("h6v6h-6z").count(),
                    214,
                    "embedded svg is valid"
                );
            }) as fn(&str),
        ),
        (
            "letters_keep_square_particles_and_rectilinear_strokes",
            (|_scenario: &str| {
                let mut count = 0;
                for letter in ["S", "a", "i", "l", "r", "y"] {
                    let dots = dots(letter);
                    count += dots.len();
                    for (x, y) in dots {
                        assert_eq!(
                            x % 8,
                            1,
                            "letters keep square particles and rectilinear strokes"
                        );
                        assert_eq!(
                            y % 8,
                            1,
                            "letters keep square particles and rectilinear strokes"
                        );
                        assert!(
                            x + 6 <= 328 && y + 6 <= 112,
                            "letters keep square particles and rectilinear strokes"
                        );
                    }
                }
                assert_eq!(
                    count, 214,
                    "letters keep square particles and rectilinear strokes"
                );
                for (letter, left, right, bars) in [
                    ("S", 1, 57, &[9, 17, 41, 49, 81, 89][..]),
                    ("a", 81, 129, &[33, 57, 81, 89][..]),
                    ("r", 217, 249, &[33, 41][..]),
                    ("y", 273, 321, &[57, 65, 97, 105][..]),
                ] {
                    let dots = dots(letter);
                    for y in bars {
                        for x in (left..=right).step_by(8) {
                            assert!(
                                dots.contains(&(x, *y)),
                                "letters keep square particles and rectilinear strokes: {letter}: ({x}, {y})"
                            );
                        }
                    }
                }
            }),
        ),
        (
            "canvas_particles_match_the_artwork",
            (|_scenario: &str| {
                let expected: std::collections::BTreeSet<_> = ["S", "a", "i", "l", "r", "y"]
                    .into_iter()
                    .flat_map(dots)
                    .collect();
                let actual: std::collections::BTreeSet<_> = particles()
                    .iter()
                    .map(|&(x, y)| {
                        assert_eq!(x.fract(), 0., "canvas particles match the artwork");
                        assert_eq!(y.fract(), 0., "canvas particles match the artwork");
                        (x as u16, y as u16)
                    })
                    .collect();
                assert_eq!(particles().len(), 214, "canvas particles match the artwork");
                assert_eq!(actual, expected, "canvas particles match the artwork");
            }),
        ),
        (
            "particles_follow_both_actual_dimensions",
            (|_scenario: &str| {
                let bounds = Bounds::new(point(px(15.), px(27.)), size(px(59.), px(20.)));
                let first = particle(bounds, 1., 9.);
                let last = particle(bounds, 321., 105.);
                let scale_x = bounds.size.width / WIDTH;
                let scale_y = bounds.size.height / HEIGHT;
                assert_ne!(scale_x, scale_y, "particles follow both actual dimensions");
                assert_eq!(
                    first.origin,
                    bounds.origin + point(scale_x, scale_y * 9.),
                    "particles follow both actual dimensions"
                );
                assert_eq!(
                    first.size,
                    size(scale_x * 6., scale_y * 6.),
                    "particles follow both actual dimensions"
                );
                assert_eq!(
                    last.origin,
                    bounds.origin + point(scale_x * 321., scale_y * 105.),
                    "particles follow both actual dimensions"
                );
                assert_eq!(
                    last.size, first.size,
                    "particles follow both actual dimensions"
                );
                assert!(
                    last.right() <= bounds.right() && last.bottom() <= bounds.bottom(),
                    "particles follow both actual dimensions"
                );
            }),
        ),
        (
            "beam_uses_horizontal_position_at_each_scale",
            (|_scenario: &str| {
                for scale in [0.5, 1., 2.] {
                    let bounds = Bounds::new(
                        point(px(100.), px(240.)),
                        size(px(WIDTH * scale), px(HEIGHT * scale)),
                    );
                    assert_eq!(
                        beam(bounds, None),
                        WIDTH / 2.,
                        "beam uses horizontal position at each scale"
                    );
                    assert_eq!(
                        beam(bounds, Some(bounds.left())),
                        0.,
                        "beam uses horizontal position at each scale"
                    );
                    assert_eq!(
                        beam(bounds, Some(bounds.center().x)),
                        WIDTH / 2.,
                        "beam uses horizontal position at each scale"
                    );
                    assert_eq!(
                        beam(bounds, Some(bounds.right())),
                        WIDTH,
                        "beam uses horizontal position at each scale"
                    );
                    assert_eq!(
                        beam(bounds, Some(bounds.left() - px(20.))),
                        0.,
                        "beam uses horizontal position at each scale"
                    );
                    assert_eq!(
                        beam(bounds, Some(bounds.right() + px(20.))),
                        WIDTH,
                        "beam uses horizontal position at each scale"
                    );
                    let moved = Bounds::new(point(bounds.left(), px(800.)), bounds.size);
                    assert_eq!(
                        beam(moved, Some(bounds.center().x)),
                        WIDTH / 2.,
                        "beam uses horizontal position at each scale"
                    );
                }
                let empty = Bounds::new(point(px(100.), px(200.)), size(px(0.), px(0.)));
                assert_eq!(
                    beam(empty, Some(px(900.))),
                    WIDTH / 2.,
                    "beam uses horizontal position at each scale"
                );
            }),
        ),
        (
            "illumination_has_soft_symmetric_falloff",
            (|_scenario: &str| {
                let center = WIDTH / 2.;
                assert_eq!(
                    strength(center, center),
                    1.,
                    "illumination has soft symmetric falloff"
                );
                let mut previous = 1.;
                for distance in [8., 16., 32., 64., 128.] {
                    let left = strength(center - distance, center);
                    let right = strength(center + distance, center);
                    assert_eq!(left, right, "illumination has soft symmetric falloff");
                    assert!(
                        right > 0. && right < previous,
                        "illumination has soft symmetric falloff"
                    );
                    previous = right;
                }
                for center in [0., WIDTH / 2., WIDTH] {
                    for &(x, _) in particles() {
                        let strength = strength(x + 3., center);
                        assert!(
                            strength.is_finite() && (0. ..=1.).contains(&strength),
                            "illumination has soft symmetric falloff"
                        );
                    }
                }
            }),
        ),
        (
            "light_changes_only_with_the_pointer",
            (|_scenario: &str| {
                let mut light = Light::default();
                assert_eq!(light.pointer, None, "light changes only with the pointer");
                assert!(light.set(px(100.)), "light changes only with the pointer");
                assert_eq!(
                    light.pointer,
                    Some(px(100.)),
                    "light changes only with the pointer"
                );
                assert!(!light.set(px(100.)), "light changes only with the pointer");
                assert!(light.set(px(200.)), "light changes only with the pointer");
                assert_eq!(
                    light.pointer,
                    Some(px(200.)),
                    "light changes only with the pointer"
                );
                assert!(!light.set(px(200.)), "light changes only with the pointer");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

fn dots(letter: &str) -> std::collections::BTreeSet<(u16, u16)> {
    let source = std::str::from_utf8(SVG).unwrap();
    let marker = format!("<path id=\"{letter}\" d=\"");
    let path = source
        .split_once(&marker)
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0;
    path.split('M')
        .skip(1)
        .map(|dot| {
            let (x, y) = dot
                .strip_suffix("h6v6h-6z")
                .unwrap()
                .split_once(' ')
                .unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect()
}
