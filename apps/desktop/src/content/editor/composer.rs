//! The conversation draft remains in Kit; only its formatted presentation changes.
use super::{State, document};
use crate::content::markdown::{
    Block, BlockKind, Cursor, Doc, Part, Selection, doc::Text, render::Annotation,
};
use gpui_kit::EntityInputHandler;
use gpui_kit::base::input::TextareaLink;
use gpui_kit::component::input::{
    Backspace, Enter, IndentInline, InputEvent, MoveDown, MoveUp, OutdentInline,
};
use gpui_kit::{App, Context, MouseDownEvent, MouseUpEvent, Window, px};
use std::ops::Range;

impl State {
    pub fn composing(&self) -> bool {
        self.composition.is_some()
    }

    pub(super) fn formatted(&self) -> bool {
        self.doc.blocks.iter().any(|block| {
            !block.containers.is_empty()
                || !matches!(&block.kind, BlockKind::Paragraph(text) if text.marks.is_empty())
        })
    }

    pub(super) fn document_active(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.formatted()
            && !self.input.textarea().is_some_and(|input| {
                input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                })
            })
    }

    fn source_range(&self) -> Range<usize> {
        let prefix = self.source_selection.is_empty()
            && self
                .map
                .offset(self.selection.head)
                .is_some_and(|offset| offset > self.source_selection.end);
        if !prefix
            && self.source_selection.end <= self.source.len()
            && self.selection
                == Selection::new(
                    self.map.cursor(self.source_selection.start, &self.doc),
                    self.map.cursor(self.source_selection.end, &self.doc),
                )
        {
            return self.source_selection.clone();
        }
        if self.selection == Selection::all(&self.doc)
            && (!self.selection.is_collapsed() || self.source_selection == (0..self.source.len()))
        {
            return 0..self.source.len();
        }
        let (start, end) = self.selection.ordered();
        self.map.offset(start).unwrap_or(0)..self.map.offset(end).unwrap_or(self.source.len())
    }

    pub(super) fn sync_selection(&mut self, cx: &mut Context<Self>) {
        if self.input.textarea().is_none() || self.composing() {
            return;
        }
        let range = self.source_range();
        self.source_selection = range.clone();
        if self.input.selection(cx) != range {
            self.input.select(range, cx);
        }
    }

    pub(super) fn select_all(&mut self, cx: &mut Context<Self>) {
        self.cancel_composition(cx);
        self.commit(cx);
        self.selection = Selection::all(&self.doc);
        if self.input.textarea().is_some() {
            self.source_selection = 0..self.source.len();
            self.input.select(self.source_selection.clone(), cx);
        }
        cx.notify();
    }

    pub(super) fn selected_source(&self) -> &str {
        self.source.get(self.source_range()).unwrap_or_default()
    }

    pub(super) fn replace_source(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_draft(value, true, window, cx);
    }

    pub(super) fn replace_draft(
        &mut self,
        value: &str,
        atomic: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        if atomic {
            self.commit(cx);
        }
        let range = self.source_range();
        if range.is_empty()
            && self.cursor().part == Part::Code
            && self.caret_text().is_some_and(|text| text.text.is_empty())
        {
            // A first code-body edit may need a line boundary or a container
            // prefix. Reuse the validated source patch for typing and paste.
            let before = self.doc.clone();
            let caret = self.doc.replace(self.selection, Text::plain(value)).caret;
            self.place(caret);
            self.stored.clear();
            self.apply(before, atomic, window, cx);
            return;
        }
        let caret = range.start + value.len();
        let mut source = self.source.to_string();
        source.replace_range(range.clone(), value);
        self.source = source.into();
        self.doc = draft(&self.source);
        self.map = super::source::Map::new(&self.source, &self.doc);
        self.selection = Selection::at(self.map.cursor(caret, &self.doc));
        self.source_selection = caret..caret;
        if !self.transaction {
            self.input.begin(cx);
            self.transaction = true;
        }
        self.input.replace(range, value.to_owned(), window, cx);
        if atomic {
            self.commit(cx);
        }
        self.stored.clear();
        self.notify_caret(cx);
    }

    pub(super) fn annotations(&self, cx: &App) -> Vec<Annotation> {
        self.input
            .textarea()
            .into_iter()
            .flat_map(|input| input.read(cx).links())
            .filter_map(|link| {
                let start = self.map.cursor(link.range.start, &self.doc);
                let end = self.map.cursor(link.range.end, &self.doc);
                (start.block == end.block && start.part == end.part).then(|| Annotation {
                    at: Cursor::new(start.block, start.part, 0),
                    range: start.offset..end.offset,
                    icon: link.icon.clone(),
                })
            })
            .collect()
    }

    fn link_at(&self, offset: usize, cx: &App) -> Option<TextareaLink> {
        self.input
            .textarea()?
            .read(cx)
            .links()
            .iter()
            .find(|link| link.range.contains(&offset))
            .cloned()
    }

    pub(super) fn press_link(&mut self, event: &MouseDownEvent, cx: &App) {
        self.pressed = if event.click_count == 1 && !event.modifiers.modified() {
            self.layouts
                .hit(event.position)
                .and_then(|at| self.map.offset(at))
                .and_then(|offset| self.link_at(offset, cx))
                .map(|link| (link, event.position))
        } else {
            None
        };
    }

    pub(super) fn release_link(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some((link, origin)) = self.pressed.take() else {
            return;
        };
        let delta = event.position - origin;
        if event.modifiers.modified()
            || !self.selection.is_collapsed()
            || delta.x.abs() > px(3.)
            || delta.y.abs() > px(3.)
        {
            return;
        }
        if self
            .layouts
            .hit(event.position)
            .and_then(|at| self.map.offset(at))
            .and_then(|offset| self.link_at(offset, cx))
            .as_ref()
            == Some(&link)
            && let Some(input) = self.input.textarea()
        {
            input.update(cx, |_, cx| cx.emit(link));
        }
    }

    pub(super) fn submit(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.readonly || self.composing() {
            return;
        }
        if action.shift {
            self.replace_source("\n", window, cx);
        }
        self.commit(cx);
        self.sync_selection(cx);
        if let Some(input) = self.input.textarea() {
            input.update(cx, |_, cx| {
                cx.emit(InputEvent::PressEnter {
                    secondary: action.secondary,
                    shift: action.shift,
                })
            });
        }
    }

    pub(super) fn composer_backspace(
        &mut self,
        _: &Backspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.backspace(&super::keys::Backspace, window, cx);
    }
    pub(super) fn composer_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(false, false, cx);
    }
    pub(super) fn composer_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(true, false, cx);
    }
    pub(super) fn composer_indent(
        &mut self,
        _: &IndentInline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Kit's auto-growing Textarea is not an indentable code editor.
        cx.propagate();
    }

    pub(super) fn composer_outdent(
        &mut self,
        _: &OutdentInline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.propagate();
    }

    pub(super) fn accessibility_value(
        &mut self,
        data: Option<&gpui_kit::accesskit::ActionData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(gpui_kit::accesskit::ActionData::Value(value)) = data
            && !self.readonly
        {
            self.select_all(cx);
            self.replace_source(value, window, cx);
        }
    }
}

pub(super) fn draft(source: &str) -> Doc {
    let mut doc = document(source);
    if !source.ends_with('\n') {
        return doc;
    }
    // CommonMark omits the empty final input row. An unfinished fence still
    // owns that row; asking the same parser about one character distinguishes
    // it from a row after a closed fence without guessing delimiter spelling.
    let inside_code = matches!(
        doc.blocks.last().map(|block| &block.kind),
        Some(BlockKind::Code { .. })
    ) && matches!(
        document(&format!("{source}a"))
            .blocks
            .last()
            .map(|block| &block.kind),
        Some(BlockKind::Code { .. })
    );
    if !inside_code {
        doc.blocks
            .push(Block::new(BlockKind::Paragraph(Text::default())));
    }
    doc
}

#[cfg(test)]
mod tests;
