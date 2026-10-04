//! Bezel's floating glass surface on the single Kit GPUI renderer.
//!
//! Ported from crabtalk/bezel 4a7505ab7c36aec3333d2dac45bcf4c8c600a5a5
//! (MIT). See third_party_licenses/bezel.md. Kit's pinned renderer had no local
//! backdrop primitive; the compatible GPUI fork supplies it. Layout, hit testing
//! and control behavior remain with the child. All colors and radii use Kit's
//! active application theme; Bezel's optics use a denser tint for readable menus.
use gpui_kit::component::{
    ActiveTheme, Theme,
    menu::{PopupMenu, PopupMenuLayout},
};
use gpui_kit::{
    AbsoluteLength, AnyElement, App, Bounds, Corners, Element, GlassEffect, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Size, Window, fill, px, quad, size,
};

pub(crate) fn init(cx: &mut App) {
    PopupMenu::set_layout(
        PopupMenuLayout {
            min_width: px(180.),
            padding: cx.theme().spacing_tokens().sm,
            item_gap: px(1.),
            row_padding: size(px(8.), px(6.)),
            icon_gap: px(10.),
            separator_spacing: px(4.),
            separator_thickness: px(1.),
        },
        cx,
    );
    gpui_kit::component::surface::set_renderer(
        |child, corners, _| Surface { child, corners }.into_any_element(),
        cx,
    );
}

struct Surface {
    child: AnyElement,
    corners: Corners<AbsoluteLength>,
}

impl Surface {
    fn layer_bounds(viewport: Size<Pixels>) -> Bounds<Pixels> {
        Bounds::new(Default::default(), viewport)
    }

    fn corners(&self, bounds: Bounds<Pixels>, rem: Pixels) -> Corners<Pixels> {
        self.corners
            .to_pixels(rem)
            .clamp_radii_for_quad_size(bounds.size)
    }
}

fn glass(theme: &Theme, size: Size<Pixels>) -> GlassEffect {
    let dark = theme.is_dark();
    let gain = 0.10;
    let extent = f32::from(size.width.min(size.height));
    // The shader adds tint to the gained backdrop rather than alpha-blending it.
    // Keep backdrop luminance contrast at ten percent in both modes, with the remaining
    // weight supplied by the theme's resolved surface color.
    let tint = theme.background.blend(theme.popover).alpha(1. - gain);
    GlassEffect {
        blur_radius: px(if dark { 4. } else { 8.9 }),
        lens: px(18.75_f32.min(extent / 2.)),
        reach: px(47.0_f32.min(extent / 2.)),
        gain,
        saturation: if dark { 2.55 } else { 4.27 },
        magnify: 1.1,
        dispersion: 0.005,
        tint,
        edge: 0.,
        edge_width: px(1.),
        edge_aa: px(0.5),
    }
}

/// These backends carry the backdrop primitive. DirectX currently does not.
const BACKDROP: bool = cfg!(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_family = "wasm"
));

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = cx.theme();
        let corners = self.corners(bounds, window.rem_size());
        // Kit clears its shadow ring when delegating surface painting. Restore
        // one crisp, layout-neutral edge using the same token as inline cards.
        let border = quad(
            bounds,
            corners,
            gpui_kit::transparent_black(),
            px(1.),
            theme.border,
            Default::default(),
        );
        let effect = glass(theme, bounds.size);
        let color = theme.popover;
        // GPUI b9b7df7 records a centered text line's layer at its unaligned
        // origin, then shifts its glyphs without expanding those layer bounds.
        // A floating surface needs a viewport ordering barrier so such glyphs
        // cannot batch above it. The fill/blur still paints only the panel.
        let layer = Self::layer_bounds(window.viewport_size());
        window.paint_layer(layer, |window| {
            if BACKDROP {
                window.paint_backdrop_blur(bounds, corners, effect);
            } else {
                window.paint_quad(fill(bounds, color).corner_radii(corners));
            }
            self.child.paint(window, cx);
            window.paint_quad(border);
        });
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests;
