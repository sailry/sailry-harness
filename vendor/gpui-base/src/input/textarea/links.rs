use super::TextareaState;
pub use crate::LinkSpan as TextareaLink;
use gpui::{Context, EventEmitter, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, px};

impl EventEmitter<TextareaLink> for TextareaState {}

// The pinned Textarea has no link callback. Reuse its shaped-text hit testing
// and selection hooks instead of adding a second editable layer over the input.
impl TextareaState {
    /// Replace link spans without changing source, selection or undo history.
    /// Edits invalidate these spans just like presentation decorations.
    pub fn set_links(&mut self, mut links: Vec<TextareaLink>, cx: &mut Context<Self>) {
        let text = self.text.to_string();
        links.retain(|link| {
            link.range.start < link.range.end && text.get(link.range.clone()).is_some()
        });
        links.sort_by_key(|link| (link.range.start, link.range.end));
        let mut end = 0;
        links.retain(|link| {
            if link.range.start < end {
                return false;
            }
            end = link.range.end;
            true
        });
        if self.extras.links != links {
            self.extras.links = links;
            self.extras.pressed = None;
            self.extras.hovered = false;
            cx.notify();
        }
    }

    /// Normalized clickable spans for the current source.
    pub fn links(&self) -> &[TextareaLink] {
        &self.extras.links
    }

    fn link_at(&self, offset: usize) -> Option<&TextareaLink> {
        if self.disabled || self.ime_marked_range.is_some() {
            return None;
        }
        self.extras
            .links
            .iter()
            .find(|link| link.range.contains(&offset))
    }

    pub(super) fn press_link(&mut self, event: &MouseDownEvent, offset: usize) {
        self.extras.pressed = if event.button == MouseButton::Left
            && event.click_count == 1
            && !event.modifiers.modified()
        {
            self.link_at(offset)
                .cloned()
                .map(|link| (link, event.position))
        } else {
            None
        };
    }

    pub(super) fn release_link(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some((link, origin)) = self.extras.pressed.take() else {
            return;
        };
        let delta = event.position - origin;
        if event.button != MouseButton::Left
            || event.modifiers.modified()
            || !self.selected_range().is_empty()
            || delta.x.abs() > px(3.)
            || delta.y.abs() > px(3.)
            || !self
                .last_bounds
                .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }
        let (offset, _) = self.index_for_mouse_position(event.position);
        if self.link_at(offset) == Some(&link) {
            cx.emit(link);
        }
    }

    pub(super) fn hover_link(
        &mut self,
        offset: usize,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if let Some((_, origin)) = &self.extras.pressed {
            let delta = event.position - *origin;
            if delta.x.abs() > px(3.) || delta.y.abs() > px(3.) {
                self.extras.pressed = None;
            }
        }
        let hovered = event.pressed_button.is_none()
            && !event.modifiers.modified()
            && self.link_at(offset).is_some();
        if self.extras.hovered != hovered {
            self.extras.hovered = hovered;
            cx.notify();
        }
    }
}
