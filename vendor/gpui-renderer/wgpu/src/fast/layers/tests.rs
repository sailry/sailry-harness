// Modified by Longbridge for gpui-fast.
//! Pixel tests of scroll layers: the same content drawn directly and through
//! layer tiles must come out byte for byte the same.
//!
//! [`Harness`] draws scenes without a window: it owns the GPU objects the
//! renderer would, built by the renderer's own constructors, and records
//! through `fast::frame` as the renderer does, into a texture it reads back.

use std::borrow::Cow;
use std::num::NonZeroU64;
use std::sync::Arc;

use anyhow::Result;
use gpui::{
    AtlasKey, AtlasTile, Bounds, ContentMask, Corners, DevicePixels, Edges, FontId, GlyphId, Hsla,
    ImageId, LayerFrame, LayerKey, MonochromeSprite, Path, PlatformAtlas, PolychromeSprite, Quad,
    RenderGlyphParams, RenderImageParams, Rgba, ScaledPixels, Scene, SceneLayers, Shadow, Size,
    SubpixelSprite, TileCoord, TransformationMatrix, Underline, layer_tile_id,
    layer_tile_texture_id, linear_color_stop, linear_gradient, point, px, rgba, size,
};

use crate::WgpuAtlas;
use crate::fast::frame::{FrameHost, FrameState, FrameTarget};
use crate::fast::layers::TileCache;
use crate::wgpu_renderer::{
    GammaParams, GlobalParams, RenderingParameters, WgpuBindGroupLayouts, WgpuPipelines,
    WgpuRendererCore,
};

#[test]
fn harness_renders_a_quad_at_the_right_pixels() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    let mut scene = Scene::default();
    scene.insert_primitive(quad(sp(4., 4., 8., 8.), Hsla::from(rgba(0xff0000ff))));
    scene.finish();
    let pixels = harness.render(&scene, device_size(32, 32), rgba(0xffffffff));
    assert_eq!(pixel(&pixels, 32, 8, 8), [255, 0, 0, 255]);
    assert_eq!(pixel(&pixels, 32, 1, 1), [255, 255, 255, 255]);
}

const TILE: u32 = 512;

#[test]
fn a_rasterized_tile_equals_the_same_content_drawn_directly() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    let direct = harness.render(
        &content(&harness, (0., 0.), no_mask().bounds, Paths::With),
        device_size(1024, 1024),
        background(),
    );
    let tiles = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let content = content(&harness, (0., 0.), no_mask().bounds, Paths::With);
    let assembled = rasterize_and_assemble(&mut harness, content, &tiles);
    assert_same_pixels(&assembled, &direct, 1024);
}

#[test]
fn tiles_of_negative_coordinates_rasterize_like_positive_ones() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    let direct = harness.render(
        &content(&harness, (0., 0.), no_mask().bounds, Paths::With),
        device_size(1024, 1024),
        background(),
    );
    let tiles = [(-1, -1), (0, -1), (-1, 0), (0, 0)];
    let shifted = content(
        &harness,
        (-(TILE as f32), -(TILE as f32)),
        no_mask().bounds,
        Paths::With,
    );
    let assembled = rasterize_and_assemble(&mut harness, shifted, &tiles);
    assert_same_pixels(&assembled, &direct, 1024);
}

#[test]
fn a_composited_layer_equals_direct_drawing() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    composite_and_compare(&mut harness, (37., -91.), Paths::Without);
}

/// Paths are drawn exactly when the content moved by an even number of
/// device pixels since its tiles were rasterized. At an odd translation an
/// antialiased path edge pixel can come out one level apart: the path
/// shader's `dpdx` and `dpdy` are differences within 2×2 pixel quads,
/// which then pair other pixels.
#[test]
fn paths_composite_exactly_at_even_translations() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    composite_and_compare(&mut harness, (38., -92.), Paths::With);
}

/// Draws a window with a viewport over a scroll layer at `translation`, once
/// from the content directly and once from the layer's tiles, twice (the
/// second frame from the cached tiles), and compares the pixels.
fn composite_and_compare(harness: &mut Harness, translation: (f32, f32), paths: Paths) {
    let window = device_size(700, 500);
    let viewport = sp(40., 30., 600., 400.);
    let panel = quad(sp(0., 0., 700., 500.), Hsla::from(background()));
    let scrollbar = quad(sp(620., 40., 12., 120.), Hsla::from(rgba(0x888888ff)));

    let mut direct = Scene::default();
    direct.insert_primitive(panel);
    add_content(&mut direct, harness, translation, viewport, paths);
    direct.insert_primitive(scrollbar);
    direct.finish();
    let expected = harness.render(&direct, window, rgba(0x000000ff));

    let key = LayerKey(5);
    let tiles = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let layer = layer_frame(
        key,
        1,
        content(harness, (0., 0.), no_mask().bounds, paths),
        &tiles,
    );
    let mut composited = Scene::default();
    composited.insert_primitive(panel);
    composited.push_layer(viewport);
    for tile in tiles {
        let bounds = layer.tile_bounds(tile);
        composited.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds: sp(
                bounds.origin.x.0 + translation.0,
                bounds.origin.y.0 + translation.1,
                TILE as f32,
                TILE as f32,
            ),
            content_mask: ContentMask { bounds: viewport },
            corner_radii: Corners::default(),
            tile: AtlasTile {
                texture_id: layer_tile_texture_id(key),
                tile_id: layer_tile_id(tile),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size: device_size(TILE as i32, TILE as i32),
                },
            },
        });
    }
    composited.pop_layer();
    composited.insert_primitive(scrollbar);
    composited.layers.frames.push(layer);
    composited.finish();
    let actual = harness.render(&composited, window, rgba(0x000000ff));
    assert_same_pixels(&actual, &expected, 700);

    // A frame that only scrolled draws the cached tiles again.
    let actual = harness.render(&composited, window, rgba(0x000000ff));
    assert_same_pixels(&actual, &expected, 700);
}

#[test]
fn dirty_tiles_are_rasterized_once_they_are_shown() {
    let mut cache = TileCache::default();
    let key = LayerKey(3);
    let all = [coord(0, 0), coord(1, 0), coord(0, 1), coord(1, 1)];
    let mut rasterized = 0;
    let mut frame =
        |cache: &mut TileCache, generation, dirty: &[TileCoord], shown: &[TileCoord]| {
            let layers = scene_layers(key, generation, dirty);
            let planned = cache.begin_frame(&layers, shown.iter().map(|tile| (key, *tile)));
            rasterized += planned.len();
            planned
                .into_iter()
                .map(|(_, tile)| tile)
                .collect::<Vec<_>>()
        };

    let [a, b, c, _] = all;
    // A new layer: only the dirty tiles shown. The others wait until they are.
    let mut shown = vec![a, b];
    shown.sort();
    assert_eq!(frame(&mut cache, 1, &all, &[a, b]), shown);
    // The same generation again: nothing.
    assert_eq!(frame(&mut cache, 1, &all, &[a, b]), vec![]);
    // The next generation dirties a held tile that is not shown: it waits.
    assert_eq!(frame(&mut cache, 2, &[b, c], &[a]), vec![]);
    // Shown again, it is rasterized, and only it.
    assert_eq!(frame(&mut cache, 2, &[b, c], &[a, b]), vec![b]);
    // A dirty tile never shown before is rasterized once it is.
    assert_eq!(frame(&mut cache, 3, &[], &[a, b, c]), vec![c]);
    // A skipped generation may have dirtied any tile: shown tiles again.
    assert_eq!(frame(&mut cache, 5, &[], &[a]), vec![a]);
    // A shown tile the cache never had.
    assert_eq!(frame(&mut cache, 5, &[], &[coord(2, 0)]), vec![coord(2, 0)]);
    assert_eq!(rasterized, 6);
}

#[test]
fn tiles_are_evicted_least_recently_composited_first_within_the_budget() {
    const SIDE: u32 = 16;
    let tile_bytes = (SIDE * SIDE * 4) as u64;
    let key = LayerKey(4);
    let mut cache = TileCache::default();
    let layers = SceneLayers {
        frames: vec![LayerFrame {
            tile_size: SIDE,
            ..layer_frame(key, 1, Scene::default(), &[])
        }],
    };
    let frame = |cache: &mut TileCache, shown: &[TileCoord], budget: u64| {
        cache.begin_frame(&layers, shown.iter().map(|tile| (key, *tile)));
        cache.evict_to_budget(budget);
    };
    let (a, b, c, d, e) = (
        coord(0, 0),
        coord(1, 0),
        coord(2, 0),
        coord(3, 0),
        coord(4, 0),
    );

    frame(&mut cache, &[a, b, c], 3 * tile_bytes);
    frame(&mut cache, &[b, c, d], 3 * tile_bytes);
    assert!(
        !cache.holds(key, a),
        "the least recently composited tile goes"
    );
    assert!(cache.holds(key, b) && cache.holds(key, c) && cache.holds(key, d));

    frame(&mut cache, &[c], 3 * tile_bytes);
    frame(&mut cache, &[d, e], tile_bytes);
    assert!(!cache.holds(key, b) && !cache.holds(key, c));
    assert!(
        cache.holds(key, d) && cache.holds(key, e),
        "tiles composited this frame stay, over budget or not"
    );
}

#[test]
fn resize_releases_every_tile() {
    let Some(mut harness) = Harness::new() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    let key = LayerKey(1);
    let tiles = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let content = content(&harness, (0., 0.), no_mask().bounds, Paths::With);
    rasterize_and_assemble(&mut harness, content, &tiles);
    assert!(harness.state().layers.holds(key, coord(0, 0)));

    // What `WgpuRenderer::update_drawable_size` calls.
    TileCache::clear(&mut harness.state().layers);
    assert!(harness.state().layers.is_empty());

    // The next frame rasterizes the tiles it composites again.
    let content = content_again(&harness);
    let assembled = rasterize_and_assemble(&mut harness, content, &tiles);
    let direct = harness.render(
        &content_again(&harness),
        device_size(1024, 1024),
        background(),
    );
    assert_same_pixels(&assembled, &direct, 1024);
}

fn content_again(harness: &Harness) -> Scene {
    content(harness, (0., 0.), no_mask().bounds, Paths::With)
}

// --- layer helpers --- //

fn coord(x: i32, y: i32) -> TileCoord {
    TileCoord { x, y }
}

fn background() -> Rgba {
    rgba(0x336699ff)
}

fn layer_frame(key: LayerKey, generation: u64, content: Scene, dirty: &[TileCoord]) -> LayerFrame {
    LayerFrame {
        key,
        generation,
        background: background(),
        tile_size: TILE,
        content: content.into(),
        dirty_tiles: dirty.to_vec(),
    }
}

fn scene_layers(key: LayerKey, generation: u64, dirty: &[TileCoord]) -> SceneLayers {
    SceneLayers {
        frames: vec![layer_frame(key, generation, Scene::default(), dirty)],
    }
}

/// Draws a frame that rasterizes `tiles` of `content` and puts the tiles
/// together into one image, `tiles[0]` at its top left.
fn rasterize_and_assemble(harness: &mut Harness, content: Scene, tiles: &[(i32, i32)]) -> Vec<u8> {
    let key = LayerKey(1);
    let coords: Vec<TileCoord> = tiles.iter().map(|&(x, y)| coord(x, y)).collect();
    // The frame composites the tiles, where they were painted: tiles are
    // rasterized once a frame shows them.
    let layer = layer_frame(key, 1, content, &coords);
    let mut frame = Scene::default();
    for &tile in &coords {
        let bounds = layer.tile_bounds(tile);
        frame.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::default(),
            tile: AtlasTile {
                texture_id: layer_tile_texture_id(key),
                tile_id: layer_tile_id(tile),
                padding: 0,
                bounds: Bounds {
                    origin: point(DevicePixels(0), DevicePixels(0)),
                    size: device_size(TILE as i32, TILE as i32),
                },
            },
        });
    }
    frame.layers.frames.push(layer);
    frame.finish();
    harness.render(&frame, device_size(16, 16), background());

    let (min_x, min_y) = (tiles[0].0, tiles[0].1);
    let side = TILE as usize;
    let width = 2 * side;
    let mut image = vec![0; width * width * 4];
    for tile in coords {
        let texture = harness
            .state()
            .layers
            .tile_texture(key, tile)
            .expect("tile rasterized")
            .clone();
        let pixels = read_back(
            harness.device(),
            harness.queue(),
            &texture,
            harness.format(),
        );
        let x0 = (tile.x - min_x) as usize * side;
        let y0 = (tile.y - min_y) as usize * side;
        for y in 0..side {
            let from = y * side * 4;
            let to = ((y0 + y) * width + x0) * 4;
            image[to..to + side * 4].copy_from_slice(&pixels[from..from + side * 4]);
        }
    }
    image
}

fn assert_same_pixels(actual: &[u8], expected: &[u8], width: usize) {
    assert_eq!(actual.len(), expected.len());
    let differing: Vec<(usize, usize)> = (0..actual.len() / 4)
        .filter(|i| actual[i * 4..i * 4 + 4] != expected[i * 4..i * 4 + 4])
        .map(|i| (i % width, i / width))
        .collect();
    if differing.is_empty() {
        return;
    }
    let (min_x, max_x) = differing
        .iter()
        .fold((usize::MAX, 0), |(lo, hi), (x, _)| (lo.min(*x), hi.max(*x)));
    let (min_y, max_y) = differing
        .iter()
        .fold((usize::MAX, 0), |(lo, hi), (_, y)| (lo.min(*y), hi.max(*y)));
    panic!(
        "{} pixels differ, within x {min_x}..={max_x}, y {min_y}..={max_y}; first at {:?}: {:?} instead of {:?}",
        differing.len(),
        differing[0],
        pixel(actual, width, differing[0].0, differing[0].1),
        pixel(expected, width, differing[0].0, differing[0].1),
    );
}

/// Content of every primitive kind, spread over the four tiles of
/// (0, 0)..(1024, 1024) and across their edges, moved by `offset` and
/// clipped to `clip`.
fn content(
    harness: &Harness,
    offset: (f32, f32),
    clip: Bounds<ScaledPixels>,
    paths: Paths,
) -> Scene {
    let mut scene = Scene::default();
    add_content(&mut scene, harness, offset, clip, paths);
    scene.finish();
    scene
}

/// Inserts [`content`]'s primitives into `scene`.
#[derive(Clone, Copy, PartialEq)]
enum Paths {
    With,
    Without,
}

fn add_content(
    scene: &mut Scene,
    harness: &Harness,
    offset: (f32, f32),
    clip: Bounds<ScaledPixels>,
    paths: Paths,
) {
    let (ox, oy) = offset;
    let at = |x: f32, y: f32, w: f32, h: f32| sp(x + ox, y + oy, w, h);
    let mask = |bounds: Bounds<ScaledPixels>| ContentMask {
        bounds: bounds.intersect(&clip),
    };

    scene.insert_primitive(Quad {
        content_mask: mask(clip),
        ..quad(at(100., 100., 200., 150.), Hsla::from(rgba(0xcc3322ff)))
    });
    scene.insert_primitive(Quad {
        bounds: at(450.5, 60., 150., 120.),
        content_mask: mask(at(0., 0., 1024., 1024.)),
        background: Hsla::from(rgba(0xeeeeeeff)).into(),
        border_color: Hsla::from(rgba(0x222222ff)),
        corner_radii: Corners::all(ScaledPixels(12.)),
        border_widths: Edges::all(ScaledPixels(3.)),
        ..Default::default()
    });
    // The shadow's tail crosses the tile edge its bounds stop short of.
    scene.insert_primitive(Shadow {
        order: 0,
        blur_radius: ScaledPixels(8.),
        bounds: at(380., 420., 120., 70.),
        corner_radii: Corners::all(ScaledPixels(6.)),
        content_mask: mask(clip),
        color: Hsla::from(rgba(0x00000080)),
        element_bounds: at(380., 420., 120., 70.),
        element_corner_radii: Corners::all(ScaledPixels(6.)),
        inset: 0,
        pad: 0,
    });
    scene.insert_primitive(Quad {
        bounds: at(600., 600., 300., 200.),
        content_mask: mask(clip),
        background: linear_gradient(
            45.,
            linear_color_stop(rgba(0xff0000ff), 0.),
            linear_color_stop(rgba(0x0000ffff), 1.),
        ),
        corner_radii: Corners::all(ScaledPixels(20.)),
        ..Default::default()
    });
    scene.insert_primitive(Underline {
        order: 0,
        pad: 0,
        bounds: at(50., 505., 900., 8.),
        content_mask: mask(clip),
        color: Hsla::from(rgba(0xffcc00ff)),
        thickness: ScaledPixels(2.),
        wavy: true.into(),
    });
    // A clipped quad whose mask crosses a tile edge.
    scene.insert_primitive(Quad {
        bounds: at(700., 200., 200., 200.),
        content_mask: mask(at(490., 250., 300., 100.)),
        background: Hsla::from(rgba(0x44aa44ff)).into(),
        ..Default::default()
    });

    if paths == Paths::With {
        add_path(scene, offset, clip);
    }

    let mono = glyph_tile(harness, 1, false);
    scene.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: at(505., 700., 16., 16.),
        content_mask: mask(clip),
        color: Hsla::from(rgba(0xffffffff)),
        tile: mono,
        transformation: TransformationMatrix::unit(),
    });
    if !harness.dual_source_blending {
        eprintln!("no dual-source blending: subpixel sprites left out");
    } else {
        let subpixel = glyph_tile(harness, 2, true);
        scene.insert_primitive(SubpixelSprite {
            order: 0,
            pad: 0,
            bounds: at(520., 505., 16., 16.),
            content_mask: mask(clip),
            color: Hsla::from(rgba(0x000000ff)),
            tile: subpixel,
            transformation: TransformationMatrix::unit(),
        });
    }
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false.into(),
        opacity: 1.,
        bounds: at(300., 500., 20., 20.),
        content_mask: mask(clip),
        corner_radii: Corners::all(ScaledPixels(4.)),
        tile: image_tile(harness),
    });
}

/// A path of lines and a curve across a tile edge.
fn add_path(scene: &mut Scene, (ox, oy): (f32, f32), clip: Bounds<ScaledPixels>) {
    let mut path = Path::new(point(px(480. + ox), px(300. + oy)));
    path.line_to(point(px(560.5 + ox), px(330. + oy)));
    path.curve_to(
        point(px(500. + ox), px(560. + oy)),
        point(px(600. + ox), px(450. + oy)),
    );
    path.line_to(point(px(480. + ox), px(300. + oy)));
    path.content_mask = ContentMask {
        bounds: clip.map(|c| px(c.0)),
    };
    path.color = Hsla::from(rgba(0x8800ffcc)).into();
    scene.insert_primitive(path.scale(1.));
}

/// A 16×16 glyph of a made-up font, uploaded to the harness's atlas: a
/// monochrome one, or a subpixel one.
fn glyph_tile(harness: &Harness, glyph: u32, subpixel: bool) -> AtlasTile {
    let key = AtlasKey::Glyph(RenderGlyphParams {
        font_id: FontId(9_999),
        glyph_id: GlyphId(glyph),
        font_size: px(12.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: subpixel,
        dilation: 0,
    });
    let bytes_per_pixel = if subpixel { 4 } else { 1 };
    let bytes: Vec<u8> = (0..16 * 16 * bytes_per_pixel)
        .map(|i| ((i * 37) % 256) as u8)
        .collect();
    harness
        .atlas
        .get_or_insert_with(key, &mut || {
            Ok(Some((device_size(16, 16), Cow::Owned(bytes.clone()))))
        })
        .expect("glyph uploaded")
        .expect("glyph tile")
}

/// A 20×20 image uploaded to the harness's atlas.
fn image_tile(harness: &Harness) -> AtlasTile {
    let key = AtlasKey::Image(RenderImageParams {
        image_id: ImageId(9_999),
        frame_index: 0,
    });
    let bytes: Vec<u8> = (0..20 * 20)
        .flat_map(|i: u32| {
            [
                (i * 7 % 256) as u8,
                (i * 13 % 256) as u8,
                (i * 3 % 256) as u8,
                255,
            ]
        })
        .collect();
    harness
        .atlas
        .get_or_insert_with(key, &mut || {
            Ok(Some((device_size(20, 20), Cow::Owned(bytes.clone()))))
        })
        .expect("image uploaded")
        .expect("image tile")
}

// --- helpers --- //

pub(super) fn sp(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds {
        origin: point(ScaledPixels(x), ScaledPixels(y)),
        size: size(ScaledPixels(w), ScaledPixels(h)),
    }
}

/// A content mask that clips nothing the tests draw.
pub(super) fn no_mask() -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: sp(-10_000., -10_000., 20_000., 20_000.),
    }
}

pub(super) fn quad(bounds: Bounds<ScaledPixels>, color: Hsla) -> Quad {
    Quad {
        bounds,
        content_mask: no_mask(),
        background: color.into(),
        ..Default::default()
    }
}

pub(super) fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
    size(DevicePixels(width), DevicePixels(height))
}

/// The RGBA bytes of the pixel at (`x`, `y`) of a `width`-wide readback.
pub(super) fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

const INITIAL_INSTANCE_CAPACITY: u64 = 2 * 1024 * 1024;

/// Draws scenes into textures without a surface, through the renderer's
/// pipelines and frame recording.
pub(super) struct Harness {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    format: wgpu::TextureFormat,
    atlas: WgpuAtlas,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    rendering_params: RenderingParameters,
    globals_buffer: wgpu::Buffer,
    path_globals_offset: u64,
    gamma_offset: u64,
    globals_bind_group: wgpu::BindGroup,
    path_globals_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    instance_alignment: u64,
    path_targets: Option<PathTargets>,
    state: FrameState,
    dual_source_blending: bool,
}

/// The path intermediate texture, and its multisampled twin, of one size.
struct PathTargets {
    size: Size<DevicePixels>,
    _intermediate: wgpu::Texture,
    intermediate_view: wgpu::TextureView,
    _msaa: Option<wgpu::Texture>,
    msaa_view: Option<wgpu::TextureView>,
}

impl Harness {
    /// A harness on the first adapter wgpu offers without a surface, or
    /// `None` when there is none (the tests then skip).
    pub(super) fn new() -> Option<Harness> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let adapter = gpui::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok()?;
        let (device, queue, dual_source_blending, color_texture_format) =
            gpui::block_on(crate::WgpuContext::create_device(&adapter)).ok()?;
        let (device, queue) = (Arc::new(device), Arc::new(queue));

        // The surface's choice when it offers both.
        let usages = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
        let format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|format| {
            adapter
                .get_texture_format_features(*format)
                .allowed_usages
                .contains(usages)
        })?;

        let rendering_params = RenderingParameters::new(&adapter, format);
        let bind_group_layouts = WgpuRendererCore::create_bind_group_layouts(&device, false);
        let pipelines = WgpuRendererCore::create_pipelines(
            &device,
            &bind_group_layouts,
            format,
            wgpu::CompositeAlphaMode::Opaque,
            rendering_params.path_sample_count,
            dual_source_blending,
            false,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = size_of::<GlobalParams>() as u64;
        let gamma_size = size_of::<GammaParams>() as u64;
        let path_globals_offset = globals_size.next_multiple_of(uniform_alignment);
        let gamma_offset = (path_globals_offset + globals_size).next_multiple_of(uniform_alignment);
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: gamma_offset + gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = |label: &str, offset: u64| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &bind_group_layouts.globals,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &globals_buffer,
                            offset,
                            size: NonZeroU64::new(globals_size),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &globals_buffer,
                            offset: gamma_offset,
                            size: NonZeroU64::new(gamma_size),
                        }),
                    },
                ],
            })
        };
        let path_globals_bind_group =
            globals_bind_group("path_globals_bind_group", path_globals_offset);
        let globals_bind_group = globals_bind_group("globals_bind_group", 0);

        let instance_alignment = device.limits().min_storage_buffer_offset_alignment as u64;
        let instance_buffer = create_instance_buffer(&device, INITIAL_INSTANCE_CAPACITY);
        let atlas = WgpuAtlas::new(device.clone(), queue.clone(), color_texture_format);

        Some(Harness {
            device,
            queue,
            format,
            atlas,
            pipelines,
            bind_group_layouts,
            atlas_sampler,
            rendering_params,
            globals_buffer,
            path_globals_offset,
            gamma_offset,
            globals_bind_group,
            path_globals_bind_group,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            instance_alignment,
            path_targets: None,
            state: FrameState::default(),
            dual_source_blending,
        })
    }

    pub(super) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub(super) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub(super) fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub(super) fn state(&mut self) -> &mut FrameState {
        &mut self.state
    }

    /// Draws `scene` into a `size` texture cleared to `clear`, as the
    /// renderer draws a frame, and returns its pixels as RGBA bytes, row by row.
    pub(super) fn render(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
        clear: Rgba,
    ) -> Vec<u8> {
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        self.write_globals(width, height);
        self.ensure_path_targets(size);
        self.atlas.before_frame();

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("harness_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let clear = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        };
        crate::fast::frame::record_into(self, scene, &view, clear).expect("frame recorded");
        read_back(&self.device, &self.queue, &texture, self.format)
    }

    fn write_globals(&self, width: u32, height: u32) {
        let globals = GlobalParams {
            viewport_size: [width as f32, height as f32],
            premultiplied_alpha: 0,
            pad: 0,
        };
        let gamma = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: 0,
            _pad: 0,
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));
        self.queue.write_buffer(
            &self.globals_buffer,
            self.path_globals_offset,
            bytemuck::bytes_of(&globals),
        );
        self.queue.write_buffer(
            &self.globals_buffer,
            self.gamma_offset,
            bytemuck::bytes_of(&gamma),
        );
    }

    fn ensure_path_targets(&mut self, size: Size<DevicePixels>) {
        if self
            .path_targets
            .as_ref()
            .is_some_and(|targets| targets.size == size)
        {
            return;
        }
        let (width, height) = (size.width.0 as u32, size.height.0 as u32);
        let (intermediate, intermediate_view) =
            WgpuRendererCore::create_path_intermediate(&self.device, self.format, width, height);
        let (msaa, msaa_view) = WgpuRendererCore::create_msaa_if_needed(
            &self.device,
            self.format,
            width,
            height,
            self.rendering_params.path_sample_count,
        )
        .unzip();
        self.path_targets = Some(PathTargets {
            size,
            _intermediate: intermediate,
            intermediate_view,
            _msaa: msaa,
            msaa_view,
        });
    }
}

impl FrameHost for Harness {
    fn frame_state(&mut self) -> &mut FrameState {
        &mut self.state
    }

    fn instance_data_alignment(&self) -> u64 {
        self.instance_alignment.max(1)
    }

    fn reserve_instance_data(&mut self, size: u64) -> Result<()> {
        if size > self.instance_capacity {
            self.instance_capacity = size.next_power_of_two();
            self.instance_buffer = create_instance_buffer(&self.device, self.instance_capacity);
        }
        Ok(())
    }

    fn target(&self) -> Result<FrameTarget<'_>> {
        let path_targets = self.path_targets.as_ref();
        Ok(FrameTarget {
            device: &self.device,
            queue: &self.queue,
            pipelines: &self.pipelines,
            bind_group_layouts: &self.bind_group_layouts,
            atlas: &self.atlas,
            atlas_sampler: &self.atlas_sampler,
            globals_bind_group: &self.globals_bind_group,
            path_globals_bind_group: &self.path_globals_bind_group,
            path_intermediate_view: path_targets.map(|targets| &targets.intermediate_view),
            path_msaa_view: path_targets.and_then(|targets| targets.msaa_view.as_ref()),
            instance_buffer: &self.instance_buffer,
            globals_buffer: &self.globals_buffer,
            gamma_offset: self.gamma_offset,
            gamma_size: size_of::<GammaParams>() as u64,
            format: self.format,
            path_sample_count: self.rendering_params.path_sample_count,
            premultiplied_alpha: false,
        })
    }
}

fn create_instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instance_buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The pixels of `texture` as RGBA bytes, row by row.
pub(super) fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    format: wgpu::TextureFormat,
) -> Vec<u8> {
    let (width, height) = (texture.width(), texture.height());
    let row = width * 4;
    let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("harness_readback"),
        size: (padded_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("harness_readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("readback mapped")
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device polled");
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((row * height) as usize);
    for y in 0..height {
        let start = (y * padded_row) as usize;
        pixels.extend_from_slice(&mapped[start..start + row as usize]);
    }
    drop(mapped);
    buffer.unmap();
    if format == wgpu::TextureFormat::Bgra8Unorm {
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    pixels
}
