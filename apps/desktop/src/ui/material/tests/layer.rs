//! Scene ordering tests, not image or screenshot assertions.
use super::*;
use gpui_kit::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, BackdropBlur, ContentMask, MonochromeSprite,
    PrimitiveBatch, Quad, ScaledPixels, Scene, TileId,
};

#[test]
fn floating_surface_orders_after_shifted_text() {
    let local = bounds(600., 0., 300., 500.);
    let viewport = Surface::layer_bounds(size(px(1000.), px(600.))).scale(1.);
    let (local_scene, greeting) = fixture(local);
    let local_blur = local_scene.backdrop_blurs[0].order;
    assert!(local_scene.monochrome_sprites[greeting].order >= local_blur);

    let (floating_scene, greeting) = fixture(viewport);
    let blur = floating_scene.backdrop_blurs[0].order;
    assert!(floating_scene.monochrome_sprites[greeting].order < blur);
    let mut crossed = false;
    for batch in floating_scene.batches() {
        crossed |= floating_scene.batch_first_order(&batch) >= blur;
        if let PrimitiveBatch::MonochromeSprites { range, .. } = batch
            && range.contains(&greeting)
        {
            assert!(
                !crossed,
                "background glyphs must be drawn before the floating material"
            );
        }
    }
}

fn fixture(layer: Bounds<ScaledPixels>) -> (Scene, usize) {
    let mut scene = Scene::default();
    let viewport = bounds(0., 0., 1000., 600.);
    let mask = ContentMask { bounds: viewport };
    let origin = bounds(0., 300., 100., 40.);
    // Center alignment moves glyphs right, but the pinned text painter registers
    // its unaligned line origin. Repeated overlapping left-side layers make the
    // erroneous greeting order greater than the local popup's order.
    for _ in 0..3 {
        scene.push_layer(origin);
        scene.insert_primitive(Quad {
            bounds: origin,
            content_mask: mask,
            ..Default::default()
        });
        scene.pop_layer();
    }
    scene.push_layer(origin);
    scene.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: bounds(650., 310., 12., 20.),
        content_mask: mask,
        color: gpui_kit::white(),
        tile: AtlasTile {
            texture_id: AtlasTextureId {
                index: 0,
                kind: AtlasTextureKind::Monochrome,
            },
            tile_id: TileId(0),
            padding: 0,
            bounds: Default::default(),
        },
        transformation: gpui_kit::TransformationMatrix::unit(),
    });
    scene.pop_layer();
    scene.push_layer(layer);
    scene.insert_backdrop_blur(BackdropBlur {
        order: 0,
        blur_radius: ScaledPixels(4.),
        bounds: bounds(600., 0., 300., 500.),
        content_mask: mask,
        corner_radii: Default::default(),
        lens: ScaledPixels(0.),
        reach: ScaledPixels(0.),
        magnify: 1.,
        dispersion: 0.,
        gain: 0.1,
        saturation: 1.,
        tint: gpui_kit::black(),
        edge: 0.,
        edge_width: ScaledPixels(1.),
        edge_aa: ScaledPixels(0.5),
        opacity: 1.,
    });
    scene.pop_layer();
    scene.finish();
    (scene, 0)
}

fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(width), ScaledPixels(height)),
    )
}
