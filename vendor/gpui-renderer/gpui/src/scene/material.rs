use super::*;
use crate::{
    fast::layers::{paint::lift_paths, tiles::tile_hashes},
    hsla, size,
};

fn bounds() -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(10.), ScaledPixels(20.)),
        size(ScaledPixels(40.), ScaledPixels(30.)),
    )
}

fn blur() -> BackdropBlur {
    let bounds = bounds();
    BackdropBlur {
        order: 0,
        blur_radius: ScaledPixels(6.),
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        lens: ScaledPixels(8.),
        reach: ScaledPixels(10.),
        magnify: 1.1,
        dispersion: 0.005,
        gain: 0.1,
        saturation: 2.5,
        tint: hsla(0., 0., 0.8, 0.9),
        edge: 0.,
        edge_width: ScaledPixels(1.),
        edge_aa: ScaledPixels(0.5),
        opacity: 1.,
    }
}

fn shadow() -> Shadow {
    let bounds = bounds();
    Shadow {
        order: 0,
        blur_radius: ScaledPixels(0.),
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Corners::default(),
        color: crate::red(),
        element_bounds: bounds,
        element_corner_radii: Corners::default(),
        inset: 0,
        pad: 0,
    }
}

#[test]
fn splits_adjacent_shadow_batches() {
    let mut scene = Scene::default();
    scene.insert_primitive(shadow());
    scene.insert_backdrop_blur(blur());
    scene.insert_primitive(shadow());
    scene.finish();
    let batches: Vec<_> = scene.batches().collect();
    assert_eq!(batches.len(), 2);
    assert!(scene.batch_first_order(&batches[0]) < scene.backdrop_blurs[0].order);
    assert_eq!(
        scene.batch_first_order(&batches[1]),
        scene.backdrop_blurs[0].order
    );
    assert_eq!(scene.shadows.len(), 3);
}

#[test]
fn replays_with_scene_order() {
    let mut source = Scene::default();
    source.insert_primitive(shadow());
    source.push_layer(bounds());
    source.insert_backdrop_blur(blur());
    source.insert_primitive(shadow());
    source.pop_layer();
    source.finish();
    let mut replayed = Scene::default();
    replayed.replay(0..source.len(), &source);
    replayed.finish();
    assert_eq!(source.describe(), replayed.describe());
    assert_eq!(replayed.backdrop_blurs.len(), 1);
}

#[test]
fn balances_composition_range() {
    let mut source = Scene::default();
    source.push_layer(bounds());
    source.insert_backdrop_blur(blur());
    source.pop_layer();
    source.finish();
    let mut overlay = Scene::default();
    overlay.replay_balanced(1..2, &source).unwrap();
    overlay.finish();
    assert_eq!(overlay.backdrop_blurs.len(), 1);
    assert_eq!(overlay.backdrop_blurs[0].content_mask.bounds, bounds());
    assert!(matches!(
        overlay.paint_operations.as_slice(),
        [
            PaintOperation::StartLayer(_),
            PaintOperation::BackdropBlur(_),
            PaintOperation::EndLayer
        ]
    ));
}

#[test]
fn excludes_empty_regions() {
    let mut scene = Scene::default();
    let mut blur = blur();
    blur.content_mask.bounds.origin.x = ScaledPixels(500.);
    scene.insert_backdrop_blur(blur);
    assert!(scene.is_empty());
    assert_eq!(scene.len(), 0);
}

#[test]
fn paints_scroll_content_directly() {
    let mut scene = Scene::default();
    scene.insert_backdrop_blur(blur());
    assert!(lift_paths(scene).is_err());
    assert!(lift_paths(Scene::default()).is_ok());
}

#[test]
fn translates_bounds_and_masks() {
    let original = blur();
    let delta = point(ScaledPixels(15.), ScaledPixels(-5.));
    let moved = original.translated(delta);
    assert_eq!(moved.bounds.origin, original.bounds.origin + delta);
    assert_eq!(
        moved.content_mask.bounds.origin,
        original.content_mask.bounds.origin + delta
    );
    assert_eq!(moved.blur_radius, original.blur_radius);
    assert_eq!(moved.tint, original.tint);
}

#[test]
fn invalidates_changed_material() {
    let mut scene = Scene::default();
    let original = blur();
    scene.insert_backdrop_blur(original);
    let before = tile_hashes(&scene, 256, bounds());
    scene.clear();
    scene.insert_backdrop_blur(BackdropBlur {
        saturation: 4.,
        ..original
    });
    let after = tile_hashes(&scene, 256, bounds());
    assert_ne!(before, after);
}
