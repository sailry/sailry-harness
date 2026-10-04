//! Application-owned painting for floating surfaces, without replacing controls.
use std::rc::Rc;

use gpui::{AbsoluteLength, AnyElement, App, Corners, Global, IntoElement, Styled};

type Renderer = Rc<dyn Fn(AnyElement, Corners<AbsoluteLength>, &App) -> AnyElement>;

struct SurfaceRenderer(Renderer);
impl Global for SurfaceRenderer {}

/// Customize the background of menus, popovers and dialogs while retaining their focus,
/// keyboard, positioning and accessibility behavior. The renderer owns the fill
/// border and shadow; the control still supplies its content and theme corner radius.
pub fn set_renderer(
    renderer: impl Fn(AnyElement, Corners<AbsoluteLength>, &App) -> AnyElement + 'static,
    cx: &mut App,
) {
    cx.set_global(SurfaceRenderer(Rc::new(renderer)));
}

pub(crate) fn is_custom(cx: &App) -> bool {
    cx.try_global::<SurfaceRenderer>().is_some()
}

/// Paint a floating composition with the same configured surface as menus and dialogs.
pub fn render(mut child: impl IntoElement + Styled, cx: &App) -> AnyElement {
    let Some(renderer) = cx.try_global::<SurfaceRenderer>() else {
        return child.into_any_element();
    };
    // Capture the control's final rounding, including caller refinements. Rem
    // lengths are resolved by the renderer against the window at paint time.
    let radii = &child.style().corner_radii;
    let corners = Corners {
        top_left: radii.top_left.unwrap_or_default(),
        top_right: radii.top_right.unwrap_or_default(),
        bottom_right: radii.bottom_right.unwrap_or_default(),
        bottom_left: radii.bottom_left.unwrap_or_default(),
    };
    let child = child
        .bg(gpui::transparent_black())
        .border_color(gpui::transparent_black())
        .shadow(Vec::new());
    (renderer.0)(child.into_any_element(), corners, cx)
}
