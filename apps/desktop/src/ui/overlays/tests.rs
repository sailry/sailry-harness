use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Bounds, VisualTestContext};

pub(crate) fn panel(cx: &mut VisualTestContext, selector: &'static str) -> gpui_kit::Quad {
    let bounds = cx
        .update(|window, _| window.try_find(selector).map(|element| element.bounds()))
        .or_else(|| cx.debug_bounds(selector))
        .expect("visible native surface");
    panel_bounds(cx, bounds)
}

fn panel_bounds(cx: &mut VisualTestContext, bounds: Bounds<gpui_kit::Pixels>) -> gpui_kit::Quad {
    cx.update(|window, _| {
        // Match the renderer's edge snapping without changing logical layout assertions.
        let bounds = Bounds::from_corners(
            bounds.origin.map(|value| window.pixel_snap(value)),
            bounds.bottom_right().map(|value| window.pixel_snap(value)),
        )
        .scale(window.scale_factor());
        window
            .painted_quads()
            .into_iter()
            .find(|quad| quad.bounds == bounds && !quad.background.is_transparent())
            .expect("the native surface must paint a theme background")
    })
}
