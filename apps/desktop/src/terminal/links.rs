use super::*;
use gpui_kit::component::scroll::ScrollbarHandle;

impl View {
    pub(super) fn hovered_link(&self, window: &Window) -> Option<String> {
        let modifiers = window.modifiers();
        if self.selection.dragging || !(modifiers.platform || modifiers.control) {
            return None;
        }
        self.link_at(window.mouse_position())
    }

    pub(super) fn pointer_cursor(&self, window: &Window) -> CursorStyle {
        if self.hovered_link(window).is_some() {
            CursorStyle::PointingHand
        } else {
            CursorStyle::IBeam
        }
    }

    pub(super) fn link_at(&self, point: Point<Pixels>) -> Option<String> {
        if !self.metrics.bounds.contains(&point) {
            return None;
        }
        let screen = &self.state.snapshot.as_ref()?.screen;
        // Links occupy cells; selection endpoints instead round to the nearest caret.
        let column =
            ((point.x - self.metrics.bounds.left()) / self.metrics.cell.width).floor() as u16;
        let row = ((point.y - self.metrics.bounds.top() - self.scroll.offset().y)
            / self.metrics.cell.height)
            .floor() as usize;
        let line = screen.scrollback.iter().chain(&screen.rows).nth(row)?;
        let target = line
            .spans
            .iter()
            .find(|span| span.column <= column && column < span.column + span.columns)?
            .hyperlink
            .as_ref()?;
        let url = url::Url::parse(target).ok()?;
        matches!(url.scheme(), "http" | "https" | "mailto" | "file").then(|| target.clone())
    }
}
