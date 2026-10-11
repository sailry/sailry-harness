// Modified by Longbridge for gpui-fast.
//! Scroll layer tiles on wgpu: the textures a layer's content is rasterized
//! into, and the passes that draw them where the scene composites them.
//! The core side is `gpui::fast::layers`; see
//! docs/superpowers/specs/2026-09-30-scroll-layers-design.md, §5.

pub(crate) mod composite;
pub(crate) mod raster;
pub(crate) mod tile_cache;

pub(crate) use tile_cache::TileCache;

#[cfg(test)]
mod tests;
