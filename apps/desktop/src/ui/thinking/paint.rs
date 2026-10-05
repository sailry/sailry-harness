// Thinking-orbs geometry originated in gpui-thinking-orbs (MIT).
// Adapted from Bezel 4a7505ab (MIT). See third_party_licenses/bezel.md.
//! Paint a backend-agnostic [`Frame`] into a GPUI window.

use crate::ui::thinking::engine::{Frame, Line};
use gpui_kit::{
    BorderStyle, Bounds, Hsla, PathBuilder, Pixels, Point, Window, point, px, quad, size,
    transparent_black,
};

#[derive(Clone, Copy)]
pub(super) struct Ink {
    pub foreground: Hsla,
    pub background: Hsla,
}

/// Preserve the original particle shading through the active theme's ink pair.
fn ink_color(white: f32, alpha: f32, ink: Ink) -> Hsla {
    ink.foreground
        .blend(ink.background.opacity(white.clamp(0.0, 1.0)))
        .opacity(alpha.clamp(0.0, 1.0))
}

/// Paint every stroke as its own `Path`, at its exact colour.
///
/// Merging segments into shared `PathBuilder`s was tried and reverted. It is
/// tempting — a single builder accepts any number of disjoint subpaths, and
/// each `Path` primitive costs GPUI a full two-pass render — but it visibly
/// brightens the connecting state's web.
///
/// The cause is that GPUI composites paths in two stages with different blend
/// modes: subpaths of one path are rasterised together into a cleared
/// intermediate texture with premultiplied-alpha blending, and that texture is
/// then blitted with `color: OVER, alpha: ADDITIVE`. So where strokes overlap,
/// merging changes which stage accumulates them, and the result does not match
/// painting each segment separately. Merging also forces a shared colour per
/// path, which costs another approximation on top.
///
/// Measured: batching saves nothing at one orb (2.0 % either way) and about one
/// point of a core at twelve. Not worth a visible change to the artwork.
fn lines(window: &mut Window, origin: Point<Pixels>, lines: &[Line], ink: Ink) {
    for l in lines {
        if l.a < 0.02 {
            continue;
        }
        let mut b = PathBuilder::stroke(px(l.w));
        b.move_to(point(origin.x + px(l.x1), origin.y + px(l.y1)));
        b.line_to(point(origin.x + px(l.x2), origin.y + px(l.y2)));
        if let Ok(path) = b.build() {
            window.paint_path(path, ink_color(l.white, l.a, ink));
        }
    }
}

/// Paint dots as rounded quads (true circles at GPU level via corner radii).
fn dots(window: &mut Window, origin: Point<Pixels>, frame: &Frame, ink: Ink, r_min: f32) {
    for d in &frame.dots {
        if d.a < 0.02 {
            continue;
        }
        let r = d.r.max(r_min);
        let diameter = r * 2.0;
        let bounds = Bounds {
            origin: point(origin.x + px(d.x - r), origin.y + px(d.y - r)),
            size: size(px(diameter), px(diameter)),
        };
        // Fully rounded corners → disk.
        window.paint_quad(quad(
            bounds,
            px(r),
            ink_color(d.white, d.a, ink),
            px(0.),
            transparent_black(),
            BorderStyle::default(),
        ));
    }
}

/// Paint a complete frame into `bounds` (top-left of the orb).
///
/// Background is fully transparent — the host supplies the substrate.
pub(super) fn frame(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    frame: &Frame,
    ink: Ink,
    r_min: f32,
) {
    // Everything is painted inside one layer, which is the single biggest win
    // in the paint path. Outside a layer, `Scene::insert_primitive` calls
    // `BoundsTree::insert` for *every* primitive to compute its draw order —
    // a bounding-volume-hierarchy insertion whose `find_max_ordering` walk
    // degenerates when primitives overlap heavily, which is exactly the case
    // here (hundreds of discs stacked inside a 64px box). Inside a layer the
    // tree is touched once and every primitive inherits the layer's order.
    //
    // Ordering is preserved: `Scene::finish` sorts with `sort_by_key`, which is
    // stable, so quads sharing the layer's order keep their insertion order —
    // and the engine already emits dots back-to-front. The painter's algorithm
    // still holds. Confirmed by pixel-diffing the golden grid against a build
    // with this call removed: every dot-only orb came out byte-identical.
    window.paint_layer(bounds, |window| {
        lines(window, bounds.origin, &frame.lines, ink);
        dots(window, bounds.origin, frame, ink, r_min);
    });
}
