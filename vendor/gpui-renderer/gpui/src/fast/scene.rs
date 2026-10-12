// Modified by Longbridge for gpui-fast.
//! Putting a finished scene in drawing order without moving its primitives
//! more than once or allocating, and scene helpers for tests: a finished scene
//! described as text, to compare two frames by, and forgetting the orderings
//! the bounds tree replays.

use crate::{
    MonochromeSprite, PaintSurface, Path, PolychromeSprite, Quad, ScaledPixels, Scene, Shadow,
    SubpixelSprite, Underline,
};
use std::mem;

/// Room to sort a scene's primitives in, kept from one frame to the next so
/// that a frame does not allocate megabytes to put what it drew in order.
#[derive(Default)]
pub(crate) struct SortScratch {
    order: Vec<u32>,
    shadows: Vec<Shadow>,
    quads: Vec<Quad>,
    paths: Vec<Path<ScaledPixels>>,
    underlines: Vec<Underline>,
    monochrome_sprites: Vec<MonochromeSprite>,
    subpixel_sprites: Vec<SubpixelSprite>,
    polychrome_sprites: Vec<PolychromeSprite>,
    surfaces: Vec<PaintSurface>,
}

/// Puts `items` in the order `key` gives, keeping the order they came in
/// among equal keys, as a stable sort does.
///
/// Sorting indices and then gathering once moves each item a single time,
/// where sorting the items themselves moves them as often as the sort needs
/// to compare them — and these items are 112 to 168 bytes each.
fn sort_by_gathering<T: Clone, K: Ord>(
    items: &mut Vec<T>,
    order: &mut Vec<u32>,
    gathered: &mut Vec<T>,
    key: impl Fn(&T) -> K,
) {
    if items.len() < 2 {
        return;
    }
    order.clear();
    order.extend(0..items.len() as u32);
    order.sort_unstable_by_key(|&index| (key(&items[index as usize]), index));
    gathered.clear();
    gathered.extend(order.iter().map(|&index| items[index as usize].clone()));
    mem::swap(items, gathered);
}

impl Scene {
    /// Forgets the orderings recorded for replaying, so the next frame orders
    /// every primitive from scratch.
    #[cfg(test)]
    pub(crate) fn forget_orderings(&mut self) {
        self.primitive_bounds.forget();
    }

    /// Everything this finished scene draws, in drawing order, as text two
    /// scenes can be compared by: each primitive with its bounds, clip, colours
    /// and ordering, and each layer's bounds. Atlas tiles are left out, since
    /// two windows need not place the same glyph in the same tile.
    #[cfg(test)]
    pub(crate) fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for operation in &self.paint_operations {
            match operation {
                crate::PaintOperation::StartLayer(bounds) => {
                    lines.push(format!("layer {bounds:?}"))
                }
                crate::PaintOperation::EndLayer => lines.push("end layer".into()),
                crate::PaintOperation::Primitive(..) => {}
            }
        }
        lines.extend(self.shadows.iter().map(|shadow| format!("{shadow:?}")));
        lines.extend(self.quads.iter().map(|quad| format!("{quad:?}")));
        lines.extend(
            self.underlines
                .iter()
                .map(|underline| format!("{underline:?}")),
        );
        lines.extend(self.monochrome_sprites.iter().map(|sprite| {
            format!(
                "monochrome sprite {} {:?} {:?} {:?}",
                sprite.order, sprite.bounds, sprite.content_mask, sprite.color
            )
        }));
        lines.extend(self.subpixel_sprites.iter().map(|sprite| {
            format!(
                "subpixel sprite {} {:?} {:?} {:?}",
                sprite.order, sprite.bounds, sprite.content_mask, sprite.color
            )
        }));
        lines.extend(self.polychrome_sprites.iter().map(|sprite| {
            format!(
                "polychrome sprite {} {:?} {:?}",
                sprite.order, sprite.bounds, sprite.content_mask
            )
        }));
        lines.extend(
            self.paths
                .iter()
                .map(|path| format!("path {} {:?}", path.order, path.bounds)),
        );
        lines
    }
}

/// What [`Scene::finish`] does: puts every primitive in drawing order.
/// Sprites of one order are grouped by the atlas texture they come from:
/// a batch draws from one texture, and tile ids, which each texture
/// numbers from zero, would interleave them.
#[inline]
pub(crate) fn sort_in_drawing_order(scene: &mut Scene) {
    let scratch = &mut scene.sort_scratch;
    macro_rules! sort {
        ($field:ident, $key:expr) => {
            sort_by_gathering(
                &mut scene.$field,
                &mut scratch.order,
                &mut scratch.$field,
                $key,
            )
        };
    }
    sort!(shadows, |shadow: &Shadow| shadow.order);
    sort!(quads, |quad: &Quad| quad.order);
    sort!(paths, |path: &Path<ScaledPixels>| path.order);
    sort!(underlines, |underline: &Underline| underline.order);
    sort!(monochrome_sprites, |sprite: &MonochromeSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    sort!(subpixel_sprites, |sprite: &SubpixelSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    sort!(polychrome_sprites, |sprite: &PolychromeSprite| (
        sprite.order,
        sprite.tile.texture_id.index,
        sprite.tile.tile_id
    ));
    sort!(surfaces, |surface: &PaintSurface| surface.order);
}
