// Modified by Longbridge for gpui-fast.
//! Rasterizing a scroll layer tile: the layer's content over the tile,
//! translated into the tile's space, drawn into the tile's texture cleared
//! to the layer's baked background, with the frame's pipelines and a
//! viewport the size of the tile.
//!
//! All position-dependent shading is relative to each primitive's own bounds,
//! and the tile is a whole number of device pixels from the content's
//! origin, so a tile holds exactly the pixels the content would have drawn
//! over the same background in the window.

use gpui::{Rgba, Scene, SceneLayers, TileCoord};

use crate::fast::frame::{
    FrameState, FrameTarget, PathBatch, PathTargets, PathUpload, SceneDraw, SceneUpload, draw_scene,
};

/// A tile to rasterize this frame, with the part of its layer's content
/// that covers it.
pub(crate) struct TileRaster {
    /// The index of the tile's layer in the frame's `SceneLayers::frames`.
    pub(crate) layer: usize,
    pub(crate) tile: TileCoord,
    pub(crate) scene: Scene,
}

/// The scenes of the tiles `planned` by `TileCache::begin_frame`.
pub(crate) fn plan(layers: &SceneLayers, planned: &[(usize, TileCoord)]) -> Vec<TileRaster> {
    planned
        .iter()
        .map(|&(layer, tile)| TileRaster {
            layer,
            tile,
            scene: layers.frames[layer].tile_scene(tile),
        })
        .collect()
}

/// The clear colour a tile over `background` starts from: the bytes an
/// opaque quad of that colour writes to the frame's non-sRGB texture.
pub(crate) fn clear_color(background: Rgba) -> wgpu::Color {
    wgpu::Color {
        r: background.r as f64,
        g: background.g as f64,
        b: background.b as f64,
        a: background.a as f64,
    }
}

/// Records the passes that rasterize `tile` into its texture, whose scene's
/// instance data is `upload` and whose paths are `path_batches` of `paths`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize(
    target: &FrameTarget,
    state: &FrameState,
    encoder: &mut wgpu::CommandEncoder,
    layers: &SceneLayers,
    tile: &TileRaster,
    upload: &SceneUpload,
    paths: &PathUpload,
    path_batches: &[PathBatch],
) {
    let layer = &layers.frames[tile.layer];
    let cache = &state.layers;
    let (Some(view), Some(globals), Some(tile_paths)) = (
        cache.texture(layer.key, tile.tile),
        cache.globals(layer.tile_size),
        cache.paths(layer.tile_size),
    ) else {
        debug_assert!(false, "a planned tile was not prepared");
        return;
    };
    draw_scene(
        target,
        state,
        encoder,
        &SceneDraw {
            scene: &tile.scene,
            upload,
            paths,
            path_batches,
            view,
            clear: clear_color(layer.background),
            label: "layer_tile_pass",
            globals: &globals.globals,
            path_targets: PathTargets {
                globals: &globals.path_globals,
                intermediate: Some(&tile_paths.intermediate),
                msaa: tile_paths.msaa.as_ref(),
            },
        },
    );
}
