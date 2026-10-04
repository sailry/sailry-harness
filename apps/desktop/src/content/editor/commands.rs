//! Editing commands adapted from Bezel's document editor (MIT).
use super::{State, keys};
use crate::content::markdown::{
    BlockKind, Cursor, Mark, Part, Selection,
    doc::{MarkSpan, Text},
    edit,
    parse::parse,
    selectable::copied,
};
use gpui_kit::{ClipboardItem, Context, Window};

impl State {
    pub(super) fn insert(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        self.sync(cx);
        if self.input.textarea().is_some() {
            self.replace_draft(value, false, window, cx);
            return;
        }
        if value.is_empty() && self.selection.is_collapsed() {
            return;
        }
        let before = self.doc.clone();
        let start = self.selection.ordered().0;
        let previous_shortcut = before
            .blocks
            .get(start.block)
            .and_then(|block| block.text_at(start.part))
            .and_then(|text| edit::shortcut(&text.text));
        let mut typed = Text::plain(value);
        let mut marks = self.doc.marks(self.selection);
        for mark in &self.stored {
            if !marks.contains(mark) {
                marks.push(mark.clone());
            }
        }
        for mark in marks {
            typed.marks.push(MarkSpan {
                range: 0..typed.text.len(),
                mark,
            });
        }
        let splice = self.doc.replace(self.selection, typed);
        self.place(splice.caret);
        if !value.is_empty() {
            self.shortcut(previous_shortcut, start.offset..start.offset + value.len());
            self.inline_rule();
        }
        self.apply(before, false, window, cx);
    }

    fn shortcut(
        &mut self,
        previous: Option<(edit::Shortcut, usize)>,
        inserted: std::ops::Range<usize>,
    ) {
        let at = self.cursor();
        if at.part != Part::Body {
            return;
        }
        let Some(text) = self.caret_text() else {
            return;
        };
        let Some((shortcut, length)) = edit::shortcut(&text.text) else {
            return;
        };
        // Existing escaped prefixes are authored text. Only an insertion that
        // actually creates a prefix may turn it into a document structure.
        if at.offset < length || inserted.start >= length || previous == Some((shortcut, length)) {
            return;
        }
        self.doc.edit_at(at, |text| text.remove(0..length));
        self.doc.set_kind(at.block, shortcut.apply(Text::default()));
        self.place(Cursor::new(at.block, Part::Body, at.offset - length));
    }

    fn inline_rule(&mut self) {
        let at = self.cursor();
        if matches!(at.part, Part::Code | Part::Caption) {
            return;
        }
        let Some((open, inner, mark)) = self
            .caret_text()
            .and_then(|text| edit::inline_rule(&text.text, at.offset))
        else {
            return;
        };
        let width = open.len();
        self.doc.edit_at(at, |text| {
            text.remove(inner.end..at.offset);
            text.remove(open);
            text.toggle(inner.start - width..inner.end - width, mark);
        });
        self.place(Cursor::new(at.block, at.part, at.offset - 2 * width));
    }

    pub(super) fn copy(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let text = if self.input.textarea().is_some() {
            self.selected_source().to_owned()
        } else {
            copied(&self.doc, self.selection)
        };
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(super) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        self.cancel_composition(cx);
        if self.input.textarea().is_some() {
            self.replace_source(&text, window, cx);
            return;
        }
        if matches!(
            self.cursor().part,
            Part::Code | Part::Caption | Part::Cell { .. }
        ) {
            self.commit(cx);
            self.insert(&text, window, cx);
            self.commit(cx);
            return;
        }
        let parsed = parse(&text);
        let plain = match parsed.blocks.as_slice() {
            [] => text.chars().all(char::is_whitespace),
            [block] if block.containers.is_empty() => {
                matches!(&block.kind, BlockKind::Paragraph(body)
                    if body.marks.is_empty() && body.text == text.trim().replace("\r\n", "\n"))
            }
            _ => false,
        };
        if plain {
            self.commit(cx);
            self.insert(&text, window, cx);
            self.commit(cx);
            return;
        }
        let before = self.doc.clone();
        let at = self.doc.splice(self.selection, parsed);
        self.place(at);
        self.apply(before, true, window, cx);
    }

    pub(super) fn toggle_mark(&mut self, mark: Mark, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        self.commit(cx);
        let leaving_code = mark == Mark::Code && self.cursor().part == Part::Code;
        if self.selection.is_collapsed() && !leaving_code {
            if let Some(index) = self.stored.iter().position(|current| *current == mark) {
                self.stored.remove(index);
            } else {
                self.stored.push(mark);
            }
            cx.notify();
            return;
        }
        let before = self.doc.clone();
        if mark == Mark::Code
            && let Some(at) = self.doc.unfence(self.selection)
        {
            self.place(at);
        } else if mark == Mark::Code && copied(&self.doc, self.selection).contains('\n') {
            let at = self.doc.fence(self.selection);
            self.place(at);
        } else {
            self.doc.toggle_mark(self.selection, mark);
        }
        self.apply(before, true, window, cx);
    }

    pub(super) fn motion(
        &mut self,
        movement: impl FnOnce(Cursor, &crate::content::markdown::Doc) -> Cursor,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        self.cancel_composition(cx);
        self.commit(cx);
        self.stored.clear();
        let at = movement(self.cursor(), &self.doc).clamp(&self.doc);
        self.selection = if extend {
            self.selection.extend_to(at)
        } else {
            Selection::at(at)
        };
        self.notify_caret(cx);
    }

    pub(super) fn vertical(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let at = self.cursor();
        let next = self
            .layouts
            .position(at)
            .and_then(|(point, _)| self.layouts.step_row(at, point, down))
            .map(|(cursor, _)| cursor)
            .unwrap_or_else(|| {
                if down {
                    at.down(&self.doc)
                } else {
                    at.up(&self.doc)
                }
            });
        self.motion(|_, _| next, extend, cx);
    }

    pub(super) fn backspace(
        &mut self,
        _: &keys::Backspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        if self.input.textarea().is_some()
            && self.selection == Selection::all(&self.doc)
            && !self.selected_source().is_empty()
        {
            self.replace_source("", window, cx);
            return;
        }
        let before = self.doc.clone();
        let at = self.cursor();
        let structural = self.selection.is_collapsed() && at.offset == 0;
        let head = if !self.selection.is_collapsed() {
            self.doc.replace(self.selection, Text::default()).caret
        } else if at.offset > 0 {
            self.doc
                .replace(Selection::new(at.left(&self.doc), at), Text::default())
                .caret
        } else {
            self.doc.merge_back(at).unwrap_or(at)
        };
        self.place(head);
        self.apply(before, structural, window, cx);
    }

    pub(super) fn delete(&mut self, _: &keys::Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        if self.input.textarea().is_some()
            && self.selection == Selection::all(&self.doc)
            && !self.selected_source().is_empty()
        {
            self.replace_source("", window, cx);
            return;
        }
        let before = self.doc.clone();
        let at = self.cursor();
        let selection = if self.selection.is_collapsed() {
            Selection::new(at, at.right(&self.doc))
        } else {
            self.selection
        };
        let head = self.doc.replace(selection, Text::default()).caret;
        self.place(head);
        self.apply(before, false, window, cx);
    }

    pub(super) fn enter(&mut self, _: &keys::Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        match self.cursor().part {
            Part::Code => return self.insert("\n", window, cx),
            // Bezel cells are single-line; Enter does not grow a table.
            Part::Cell { .. } => return,
            _ => {}
        }
        let before = self.doc.clone();
        if !self.selection.is_collapsed() {
            let at = self.doc.replace(self.selection, Text::default()).caret;
            self.place(at);
        }
        let at = self.cursor();
        let index = self.doc.split(at.block, at.offset);
        self.place(Cursor::new(index, Part::Body, 0));
        self.apply(before, true, window, cx);
    }

    pub(super) fn soft_break(
        &mut self,
        _: &keys::SoftBreak,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.cursor().part, Part::Body | Part::Code) {
            self.insert("\n", window, cx);
        }
    }

    pub(super) fn indent(&mut self, _: &keys::Indent, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        if self.cursor().part == Part::Code {
            self.insert("    ", window, cx);
            return;
        }
        self.cancel_composition(cx);
        let before = self.doc.clone();
        self.doc.indent(self.cursor().block);
        self.apply(before, true, window, cx);
    }
    pub(super) fn outdent(
        &mut self,
        _: &keys::Outdent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        let before = self.doc.clone();
        self.doc.outdent(self.cursor().block);
        self.apply(before, true, window, cx);
    }

    pub(super) fn toggle_task(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        let before = self.doc.clone();
        if let Some(BlockKind::Task { checked, .. }) =
            self.doc.blocks.get_mut(index).map(|block| &mut block.kind)
        {
            *checked = !*checked;
        }
        self.apply(before, true, window, cx);
    }

    pub(super) fn tail_click(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.readonly {
            return false;
        }
        let Some(last) = self.doc.blocks.len().checked_sub(1) else {
            return false;
        };
        if self.doc.blocks[last].parts().last() == Some(&Part::Body)
            || !self
                .layouts
                .block_bounds(last)
                .is_some_and(|bounds| position.y > bounds.bottom())
        {
            return false;
        }
        let before = self.doc.clone();
        self.doc
            .blocks
            .push(crate::content::markdown::Block::new(BlockKind::Paragraph(
                Text::default(),
            )));
        self.place(Cursor::new(last + 1, Part::Body, 0));
        self.apply(before, true, window, cx);
        true
    }
}
