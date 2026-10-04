use super::*;
use core::prelude::v1::test;

#[test]
fn embedded_svg_is_valid() {
    let renderer = SvgRenderer::new(std::sync::Arc::new(()));
    renderer.parse_svg(SVG).unwrap();
    let source = std::str::from_utf8(SVG).unwrap();
    assert!(source.contains("viewBox=\"0 0 328 112\""));
    assert_eq!(source.matches("h6v6h-6z").count(), 214);
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

#[test]
fn letters_keep_square_particles_and_rectilinear_strokes() {
    let mut count = 0;
    for letter in ["S", "a", "i", "l", "r", "y"] {
        let dots = dots(letter);
        count += dots.len();
        for (x, y) in dots {
            assert_eq!(x % 8, 1);
            assert_eq!(y % 8, 1);
            assert!(x + 6 <= 328 && y + 6 <= 112);
        }
    }
    assert_eq!(count, 214);
    for (letter, left, right, bars) in [
        ("S", 1, 57, &[9, 17, 41, 49, 81, 89][..]),
        ("a", 81, 129, &[33, 57, 81, 89][..]),
        ("r", 217, 249, &[33, 41][..]),
        ("y", 273, 321, &[57, 65, 97, 105][..]),
    ] {
        let dots = dots(letter);
        for y in bars {
            for x in (left..=right).step_by(8) {
                assert!(dots.contains(&(x, *y)), "{letter}: ({x}, {y})");
            }
        }
    }
}

#[test]
fn canvas_particles_match_the_artwork() {
    let expected: std::collections::BTreeSet<_> = ["S", "a", "i", "l", "r", "y"]
        .into_iter()
        .flat_map(dots)
        .collect();
    let actual: std::collections::BTreeSet<_> = particles()
        .iter()
        .map(|&(x, y)| {
            assert_eq!(x.fract(), 0.);
            assert_eq!(y.fract(), 0.);
            (x as u16, y as u16)
        })
        .collect();
    assert_eq!(particles().len(), 214);
    assert_eq!(actual, expected);
}

#[test]
fn particles_follow_both_actual_dimensions() {
    let bounds = Bounds::new(point(px(15.), px(27.)), size(px(59.), px(20.)));
    let first = particle(bounds, 1., 9.);
    let last = particle(bounds, 321., 105.);
    let scale_x = bounds.size.width / WIDTH;
    let scale_y = bounds.size.height / HEIGHT;
    assert_ne!(scale_x, scale_y);
    assert_eq!(first.origin, bounds.origin + point(scale_x, scale_y * 9.));
    assert_eq!(first.size, size(scale_x * 6., scale_y * 6.));
    assert_eq!(
        last.origin,
        bounds.origin + point(scale_x * 321., scale_y * 105.)
    );
    assert_eq!(last.size, first.size);
    assert!(last.right() <= bounds.right() && last.bottom() <= bounds.bottom());
}

#[test]
fn beam_uses_horizontal_position_at_each_scale() {
    for scale in [0.5, 1., 2.] {
        let bounds = Bounds::new(
            point(px(100.), px(240.)),
            size(px(WIDTH * scale), px(HEIGHT * scale)),
        );
        assert_eq!(beam(bounds, None), WIDTH / 2.);
        assert_eq!(beam(bounds, Some(bounds.left())), 0.);
        assert_eq!(beam(bounds, Some(bounds.center().x)), WIDTH / 2.);
        assert_eq!(beam(bounds, Some(bounds.right())), WIDTH);
        assert_eq!(beam(bounds, Some(bounds.left() - px(20.))), 0.);
        assert_eq!(beam(bounds, Some(bounds.right() + px(20.))), WIDTH);
        let moved = Bounds::new(point(bounds.left(), px(800.)), bounds.size);
        assert_eq!(beam(moved, Some(bounds.center().x)), WIDTH / 2.);
    }
    let empty = Bounds::new(point(px(100.), px(200.)), size(px(0.), px(0.)));
    assert_eq!(beam(empty, Some(px(900.))), WIDTH / 2.);
}

#[test]
fn illumination_has_soft_symmetric_falloff() {
    let center = WIDTH / 2.;
    assert_eq!(strength(center, center), 1.);
    let mut previous = 1.;
    for distance in [8., 16., 32., 64., 128.] {
        let left = strength(center - distance, center);
        let right = strength(center + distance, center);
        assert_eq!(left, right);
        assert!(right > 0. && right < previous);
        previous = right;
    }
    for center in [0., WIDTH / 2., WIDTH] {
        for &(x, _) in particles() {
            let strength = strength(x + 3., center);
            assert!(strength.is_finite() && (0. ..=1.).contains(&strength));
        }
    }
}

#[test]
fn light_changes_only_with_the_pointer() {
    let mut light = Light::default();
    assert_eq!(light.pointer, None);
    assert!(light.set(px(100.)));
    assert_eq!(light.pointer, Some(px(100.)));
    assert!(!light.set(px(100.)));
    assert!(light.set(px(200.)));
    assert_eq!(light.pointer, Some(px(200.)));
    assert!(!light.set(px(200.)));
}
