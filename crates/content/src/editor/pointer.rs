//! Pointer editing adapted from Bezel 4a7505ab (MIT).
//! See third_party_licenses/bezel.md.

use super::State;
use crate::markdown::{
    Selection,
    pointer::{edge_delta, word},
};
use gpui_kit::{Context, MouseDownEvent, Pixels, Point, Window, point, px};

impl State {
    pub(super) fn press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit(cx);
        self.cancel_composition(cx);
        self.stored.clear();
        self.focus.focus(window, cx);
        self.press_link(event, cx);
        self.pointer = event.position;
        self.dragging = false;

        if let Some(index) = (0..self.doc.blocks.len()).find(|index| {
            self.layouts
                .checkbox_bounds(*index)
                .is_some_and(|bounds| bounds.contains(&event.position))
        }) {
            self.toggle_task(index, window, cx);
            return;
        }
        if self.tail_click(event.position, window, cx) {
            return;
        }
        let Some(cursor) = self.layouts.hit(event.position) else {
            return;
        };
        let cursor = cursor.clamp(&self.doc);
        self.dragging = event.click_count == 1;
        self.selection = if event.modifiers.shift {
            self.selection.extend_to(cursor)
        } else if event.click_count >= 3 {
            Selection::new(cursor.home(), cursor.end(&self.doc))
        } else if event.click_count == 2 {
            word(&self.doc, cursor)
        } else {
            Selection::at(cursor)
        };
        self.notify_caret(cx);
    }

    pub(super) fn moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let previous = (self.pointer, self.selection);
        self.pointer = position;
        if let Some(cursor) = self.layouts.hit(position) {
            self.selection = self.selection.extend_to(cursor.clamp(&self.doc));
        }
        if previous != (self.pointer, self.selection) {
            self.sync_selection(cx);
            cx.notify();
        }
    }

    pub(super) fn release(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.dragging) {
            cx.notify();
        }
    }

    pub(super) fn autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let bounds = self.scroll.bounds();
        if bounds.size.height <= px(0.) {
            return;
        }
        let delta = edge_delta(
            self.pointer.y.into(),
            bounds.top().into(),
            bounds.bottom().into(),
        );
        if delta == 0. {
            return;
        }
        let before = self.scroll.offset();
        let after = point(
            before.x,
            (before.y - px(delta)).clamp(-self.scroll.max_offset().y, px(0.)),
        );
        if before != after {
            // Keep a document drag inside its own viewport at either edge.
            self.scroll.set_offset(after);
        }
        let selected = self.selection;
        // Re-hit the final painted layouts even when the scroll limit has been
        // reached, so the final visible line participates in the selection.
        self.moved(self.pointer, cx);
        if before != after && selected == self.selection {
            cx.notify();
        }
    }
}
