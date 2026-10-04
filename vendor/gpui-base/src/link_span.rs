//! Link adornments for the pinned plain-text controls, which lack inline elements.
//! Source offsets, text selection and editing remain owned by the original controls.
use gpui::{App, Bounds, Hsla, Pixels, SharedString, Window, px, size};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct LinkSpan {
    pub range: Range<usize>,
    pub id: SharedString,
    /// SVG replacing the first source character without changing text offsets.
    pub icon: Option<SharedString>,
}

impl LinkSpan {
    pub fn marker_range(&self, text: &str) -> Option<Range<usize>> {
        let marker = text.get(self.range.clone())?.chars().next()?;
        Some(self.range.start..self.range.start + marker.len_utf8())
    }

    pub(crate) fn paint_icon(
        &self,
        bounds: Bounds<Pixels>,
        color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(icon) = self.icon.as_ref() else {
            return;
        };
        let edge = (bounds.size.width - px(1.))
            .min(bounds.size.height * 0.8)
            .max(px(0.));
        let bounds = Bounds::new(
            bounds.center() - gpui::point(edge / 2., edge / 2.),
            size(edge, edge),
        );
        _ = window.paint_svg(
            bounds,
            icon.clone(),
            None,
            gpui::TransformationMatrix::default(),
            color,
            cx,
        );
    }
}
