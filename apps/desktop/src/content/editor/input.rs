//! Platform input adapted from Bezel 4a7505ab, editor/input.rs (MIT).
//! See third_party_licenses/bezel.md. Composition is presentation state; only
//! its committed replacement reaches the caller's Kit source and history.
use std::ops::Range;

use gpui_kit::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, px, size,
};

use super::{Composition, Mode, State};
use crate::content::markdown::{Cursor, Selection, doc::Text};

impl EntityInputHandler for State {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = &self.caret_text()?.text;
        let range = from_utf16(text, range);
        *adjusted = Some(to_utf16(text, range.clone()));
        Some(text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (start, end) = self.selection.ordered();
        let same_part = start.block == end.block && start.part == end.part;
        let head = self.selection.head;
        let range = if same_part {
            start.offset..end.offset
        } else {
            head.offset..head.offset
        };
        Some(UTF16Selection {
            range: to_utf16(&self.caret_text()?.text, range),
            reversed: same_part && head == start,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        Some(to_utf16(&self.caret_text()?.text, self.marked.clone()?))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly || self.mode != Mode::Document {
            self.cancel_composition(cx);
            return;
        }
        let value = self
            .marked
            .clone()
            .and_then(|range| self.caret_text()?.text.get(range).map(str::to_owned));
        let Some(composition) = self.composition.take() else {
            self.marked = None;
            return;
        };
        self.doc = composition.doc;
        self.selection = composition.selection;
        self.marked = None;
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            self.insert(&value, window, cx);
        } else {
            self.notify_caret(cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly || self.mode != Mode::Document {
            return;
        }
        if let Some(composition) = self.composition.take() {
            self.doc = composition.doc;
            self.selection = composition.selection;
            self.marked = None;
            if text.is_empty() {
                self.notify_caret(cx);
                return;
            }
        } else if let Some(range) = range {
            self.select_platform_range(range);
        }
        self.marked = None;
        self.insert(text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly || self.mode != Mode::Document {
            return;
        }
        if let Some(range) = range {
            self.select_platform_range(range);
        } else if let Some(range) = self.marked.clone() {
            let at = self.cursor();
            self.selection = Selection::new(
                Cursor {
                    offset: range.start,
                    ..at
                },
                Cursor {
                    offset: range.end,
                    ..at
                },
            );
        }
        if self.composition.is_none() {
            self.composition = Some(Composition {
                doc: self.doc.clone(),
                selection: self.selection,
            });
        }
        if text.is_empty() {
            self.cancel_composition(cx);
            return;
        }
        let start = self.selection.clamp(&self.doc).ordered().0;
        // Candidate text bypasses Markdown shortcuts and all persistent edits.
        self.doc.replace(self.selection, Text::plain(text));
        self.marked = Some(start.offset..start.offset + text.len());
        let selected = selected
            .map(|range| from_utf16(text, range))
            .unwrap_or(text.len()..text.len());
        self.selection = Selection::new(
            Cursor {
                offset: start.offset + selected.start,
                ..start
            },
            Cursor {
                offset: start.offset + selected.end,
                ..start
            },
        );
        self.notify_caret(cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = from_utf16(&self.caret_text()?.text, range);
        let at = self.cursor();
        let (origin, height) = self.layouts.position(Cursor {
            offset: range.start,
            ..at
        })?;
        let end = self
            .layouts
            .position(Cursor {
                offset: range.end,
                ..at
            })
            .map(|(point, _)| point)
            .filter(|point| point.y == origin.y);
        Some(Bounds::new(
            origin,
            size(end.map_or(px(0.), |end| end.x - origin.x), height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let hit = self.layouts.hit(point)?;
        let at = self.cursor();
        let text = &self.caret_text()?.text;
        (hit.block == at.block && hit.part == at.part).then(|| byte_to_utf16(text, hit.offset))
    }
}

impl State {
    fn select_platform_range(&mut self, range: Range<usize>) {
        let Some(text) = self.caret_text() else {
            return;
        };
        let range = from_utf16(&text.text, range);
        let at = self.cursor();
        self.selection = Selection::new(
            Cursor {
                offset: range.start,
                ..at
            },
            Cursor {
                offset: range.end,
                ..at
            },
        )
        .clamp(&self.doc);
    }
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut bytes = 0;
    let mut units = 0;
    for ch in text.chars() {
        if units >= offset {
            break;
        }
        units += ch.len_utf16();
        bytes += ch.len_utf8();
    }
    bytes
}

fn byte_to_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    let mut bytes = 0;
    for ch in text.chars() {
        if bytes >= offset {
            break;
        }
        bytes += ch.len_utf8();
        units += ch.len_utf16();
    }
    units
}

fn from_utf16(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf16_to_byte(text, range.start);
    start..utf16_to_byte(text, range.end).max(start)
}

fn to_utf16(text: &str, range: Range<usize>) -> Range<usize> {
    byte_to_utf16(text, range.start)..byte_to_utf16(text, range.end)
}

#[cfg(test)]
mod tests;
