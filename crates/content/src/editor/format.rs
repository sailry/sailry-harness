//! Toolbar formatting state and commands for document and source modes.
use super::{Mode, State, markup};
use crate::markdown::{
    Block, BlockKind, Cursor, Doc, Mark, Part, Selection,
    doc::{Container, QuoteKind, Text},
};
use gpui_kit::{App, Context, Focusable, Window};

/// Inline formatting a toolbar can toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Italic,
    Strike,
    Code,
}

impl Style {
    pub(super) fn mark(self) -> Mark {
        match self {
            Self::Bold => Mark::Bold,
            Self::Italic => Mark::Italic,
            Self::Strike => Mark::Strike,
            Self::Code => Mark::Code,
        }
    }

    pub(super) fn delimiter(self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italic => "*",
            Self::Strike => "~~",
            Self::Code => "`",
        }
    }
}

/// A text block's kind as the toolbar names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Paragraph,
    Heading(u8),
    Bullet,
    Ordered,
    Task,
    Quote,
    Callout,
    Code,
}

impl Kind {
    /// Lists and quotes toggle back to a paragraph when chosen again.
    fn toggles(self) -> bool {
        matches!(
            self,
            Self::Bullet | Self::Ordered | Self::Task | Self::Quote
        )
    }

    fn of(block: &Block) -> Option<Self> {
        Some(match &block.kind {
            BlockKind::Paragraph(_) => match quote(block) {
                Some(Some(_)) => Self::Callout,
                Some(None) => Self::Quote,
                None => Self::Paragraph,
            },
            BlockKind::Heading { level, .. } => Self::Heading(*level),
            BlockKind::Bullet(_) => Self::Bullet,
            BlockKind::Ordered { .. } => Self::Ordered,
            BlockKind::Task { .. } => Self::Task,
            BlockKind::Quote { kind: Some(_), .. } => Self::Callout,
            BlockKind::Quote { kind: None, .. } => Self::Quote,
            BlockKind::Code { .. } => Self::Code,
            _ => return None,
        })
    }

    fn block(self, text: Text) -> BlockKind {
        match self {
            Self::Paragraph => BlockKind::Paragraph(text),
            Self::Heading(level) => BlockKind::Heading {
                level: level.clamp(1, 6),
                text,
            },
            Self::Bullet => BlockKind::Bullet(text),
            Self::Ordered => BlockKind::Ordered { number: 1, text },
            Self::Task => BlockKind::Task {
                checked: false,
                text,
            },
            Self::Quote => BlockKind::Quote { kind: None, text },
            Self::Callout => BlockKind::Quote {
                kind: Some(QuoteKind::Note),
                text,
            },
            Self::Code => BlockKind::Code {
                language: None,
                code: Text::plain(text.text),
            },
        }
    }
}

/// A block the toolbar inserts after the caret's block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Insert {
    Table,
    Image,
    Rule,
    Code,
    Mermaid,
    Math,
    Chart,
    Html,
}

impl Insert {
    /// The block's Markdown and the caret offset inside it.
    pub(super) fn snippet(self) -> (&'static str, usize) {
        match self {
            Self::Table => ("|  |  |  |\n| --- | --- | --- |\n|  |  |  |\n|  |  |  |", 2),
            Self::Image => ("![]()", 2),
            Self::Rule => ("---", 3),
            Self::Code => ("```\n\n```", 4),
            Self::Mermaid => ("```mermaid\nflowchart TD\n    A --> B\n```", 11),
            Self::Math => ("$$\n\n$$", 3),
            Self::Chart => (
                "```chart\n{\"type\":\"bar\",\"data\":[{\"label\":\"A\",\"value\":1},{\"label\":\"B\",\"value\":2}]}\n```",
                9,
            ),
            Self::Html => ("<div>\n</div>", 5),
        }
    }
}

/// What the caret or selection currently carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Format {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
    /// `None` for blocks without convertible text, such as tables and images.
    pub kind: Option<Kind>,
}

fn quote(block: &Block) -> Option<Option<QuoteKind>> {
    block
        .containers
        .iter()
        .rev()
        .find_map(|container| match container {
            Container::Quote(kind) => Some(*kind),
            Container::List => None,
        })
}

/// Convert one block, keeping its text. Quote containers are dropped when the
/// block leaves a quote or becomes a callout.
pub(super) fn retype(doc: &mut Doc, index: usize, kind: Kind) {
    let Some(current) = doc.blocks.get(index).and_then(Kind::of) else {
        return;
    };
    // A block already inside a quote stays there when Quote is chosen again.
    if current == kind || (kind == Kind::Quote && quote(&doc.blocks[index]).is_some()) {
        return;
    }
    if (kind == Kind::Paragraph && matches!(current, Kind::Quote | Kind::Callout))
        || kind == Kind::Callout
    {
        doc.blocks[index]
            .containers
            .retain(|container| !matches!(container, Container::Quote(_)));
    }
    // `set_kind` carries the existing text into the new body or code.
    doc.set_kind(index, kind.block(Text::default()));
    if matches!(kind, Kind::Heading(_))
        && let Some(text) = doc.blocks[index].text_at_mut(Part::Body)
    {
        crate::markdown::parse::collapse_to_one_line(text);
    }
}

fn convert(doc: &mut Doc, selection: Selection, kind: Kind) -> Selection {
    let current = doc.blocks.get(selection.head.block).and_then(Kind::of);
    let kind = if kind.toggles() && current == Some(kind) {
        Kind::Paragraph
    } else {
        kind
    };
    let (start, end) = selection.ordered();
    if kind == Kind::Code && start.block != end.block {
        return Selection::at(doc.fence(selection));
    }
    if kind == Kind::Paragraph
        && current == Some(Kind::Code)
        && let Some(at) = doc.unfence(selection)
    {
        return Selection::at(at);
    }
    for index in start.block..=end.block {
        retype(doc, index, kind);
    }
    if kind == Kind::Ordered {
        number_lists(doc, start.block..=end.block);
    }
    let caret = selection.head;
    let part = doc
        .blocks
        .get(caret.block)
        .and_then(|block| block.parts().first().copied())
        .unwrap_or_default();
    Selection::at(Cursor::new(caret.block, part, caret.offset)).clamp(doc)
}

/// Continue each converted list from its previous sibling, including any ordered
/// tail joined by the conversion. Nested lists keep their own starting number.
fn number_lists(doc: &mut Doc, selected: std::ops::RangeInclusive<usize>) {
    let mut changed = std::collections::BTreeSet::new();
    for index in *selected.start()..doc.blocks.len() {
        let block = &doc.blocks[index];
        let BlockKind::Ordered { number, .. } = block.kind else {
            continue;
        };
        let previous = (0..index)
            .rev()
            .find(|previous| doc.blocks[*previous].indent <= block.indent)
            .filter(|previous| doc.blocks[*previous].containers == block.containers)
            .and_then(|previous| match doc.blocks[previous].kind {
                BlockKind::Ordered { number, .. } => Some((previous, number)),
                _ => None,
            });
        if selected.contains(&index)
            || previous.is_some_and(|(previous, _)| changed.contains(&previous))
        {
            let next = previous.map_or(number, |(_, number)| number.saturating_add(1));
            if let BlockKind::Ordered { number, .. } = &mut doc.blocks[index].kind {
                *number = next;
            }
            changed.insert(index);
        }
    }
}

impl State {
    /// The caret's formatting for toolbar selection and enablement.
    pub fn format(&self, cx: &App) -> Format {
        let selection = self.format_selection(cx);
        let mut marks = self.doc.marks(selection);
        if self.mode == Mode::Document && selection.is_collapsed() {
            marks.extend(self.stored.iter().cloned());
        }
        Format {
            bold: marks.contains(&Mark::Bold),
            italic: marks.contains(&Mark::Italic),
            strike: marks.contains(&Mark::Strike),
            code: marks.contains(&Mark::Code),
            link: marks.iter().any(|mark| matches!(mark, Mark::Link(_))),
            kind: self.doc.blocks.get(selection.head.block).and_then(Kind::of),
        }
    }

    fn format_selection(&self, cx: &App) -> Selection {
        if self.mode == Mode::Document {
            self.selection
        } else {
            let range = self.input.selection(cx);
            Selection::new(
                self.map.cursor(range.start, &self.doc),
                self.map.cursor(range.end, &self.doc),
            )
        }
    }

    pub(super) fn style(&mut self, style: Style, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Document => self.toggle_mark(style.mark(), window, cx),
            Mode::Source => {
                let source = self.input.value(cx);
                let edit = markup::wrap(&source, self.input.selection(cx), style);
                self.source_edit(edit, window, cx);
            }
        }
    }

    pub(super) fn convert(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        self.sync(cx);
        self.selection = self.format_selection(cx);
        let before = self.doc.clone();
        self.selection = convert(&mut self.doc, self.selection, kind);
        self.apply(before, window, cx);
        if self.mode == Mode::Source {
            let (start, end) = self.selection.ordered();
            if let Some((start, end)) = self.map.offset(start).zip(self.map.offset(end)) {
                self.input.select(start..end, cx);
            }
        }
        self.focus_handle(cx).focus(window, cx);
    }

    pub(super) fn insert_block(
        &mut self,
        insert: Insert,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        self.sync(cx);
        let source = self.source.to_string();
        let at = match self.mode {
            Mode::Document => self
                .map
                .block_range(self.cursor().block)
                .map_or(source.len(), |range| range.end),
            Mode::Source => markup::line_end(&source, self.input.selection(cx).end),
        };
        let edit = markup::place(&source, at, insert);
        self.source_edit(Some(edit), window, cx);
        if insert == Insert::Image && self.mode == Mode::Document {
            self.edit_image(self.cursor().block, window, cx);
        }
    }

    /// Apply a source edit through Kit's buffer and project the caret back.
    pub(super) fn source_edit(
        &mut self,
        edit: Option<markup::Edit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = edit else {
            return;
        };
        if self.readonly {
            return;
        }
        self.input.replace(edit.range, edit.text, window, cx);
        self.input.select(edit.selection, cx);
        self.sync(cx);
        if self.mode == Mode::Document {
            self.selection =
                Selection::at(self.map.cursor(self.input.selection(cx).end, &self.doc));
            self.notify_caret(cx);
        }
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    fn kinds(doc: &Doc) -> Vec<Option<Kind>> {
        doc.blocks.iter().map(Kind::of).collect()
    }

    #[test]
    fn reads_quotes_and_callouts_from_containers() {
        let doc = parse("> quoted\n\n> [!NOTE]\n> noted\n\n| a |\n| - |\n| b |");
        assert_eq!(
            kinds(&doc),
            vec![Some(Kind::Quote), Some(Kind::Callout), None]
        );
    }

    mod retype {
        use super::*;

        #[test]
        fn converts_paragraph_to_heading() {
            let mut doc = parse("title");
            retype(&mut doc, 0, Kind::Heading(2));
            assert!(matches!(
                &doc.blocks[0].kind,
                BlockKind::Heading { level: 2, text } if text.text == "title"
            ));
        }

        #[test]
        fn leaves_quote_for_paragraph() {
            let mut doc = parse("> quoted");
            retype(&mut doc, 0, Kind::Paragraph);
            assert_eq!(kinds(&doc), vec![Some(Kind::Paragraph)]);
            assert!(doc.blocks[0].containers.is_empty());
        }

        #[test]
        fn makes_callout_from_paragraph() {
            let mut doc = parse("plain");
            retype(&mut doc, 0, Kind::Callout);
            assert!(matches!(
                &doc.blocks[0].kind,
                BlockKind::Quote { kind: Some(QuoteKind::Note), text } if text.text == "plain"
            ));
        }

        #[test]
        fn lists_keep_text() {
            let mut doc = parse("item");
            retype(&mut doc, 0, Kind::Task);
            assert!(matches!(
                &doc.blocks[0].kind,
                BlockKind::Task { checked: false, text } if text.text == "item"
            ));
            retype(&mut doc, 0, Kind::Paragraph);
            assert_eq!(kinds(&doc), vec![Some(Kind::Paragraph)]);
        }

        #[test]
        fn ignores_tables() {
            let mut doc = parse("| a |\n| - |\n| b |");
            let before = doc.clone();
            retype(&mut doc, 0, Kind::Heading(1));
            assert_eq!(doc, before);
        }
    }

    #[test]
    fn snippets_place_caret_inside_body() {
        for insert in [
            Insert::Table,
            Insert::Image,
            Insert::Rule,
            Insert::Code,
            Insert::Mermaid,
            Insert::Math,
            Insert::Chart,
            Insert::Html,
        ] {
            let (body, caret) = insert.snippet();
            assert!(caret <= body.len(), "{insert:?}");
            assert!(body.is_char_boundary(caret), "{insert:?}");
        }
        let (chart, caret) = Insert::Chart.snippet();
        let json = chart[caret..].strip_suffix("\n```").unwrap();
        assert!(sailry_protocol::chart::Chart::parse(json).is_some());
    }

    #[test]
    fn mapped_conversion_preserves_literal_code() {
        for (source, range, kind) in [
            (
                "```python\n# comment\nprint(1)\n```",
                0..32,
                Kind::Paragraph,
            ),
            (
                "```python\n# comment\nprint(1)\n```",
                15..15,
                Kind::Heading(2),
            ),
            ("> quoted\n> continued", 3..8, Kind::Heading(2)),
            ("- first\n- second", 2..15, Kind::Ordered),
            (
                "- first\n  - nested\n- second",
                0..usize::MAX,
                Kind::Ordered,
            ),
            ("5. first\n- second", 10..usize::MAX, Kind::Ordered),
            ("- first\n3. second", 2..3, Kind::Ordered),
        ] {
            let before = parse(source);
            let map = super::super::source::Map::new(source, &before);
            let selection = Selection::new(
                map.cursor(range.start, &before),
                map.cursor(range.end.min(source.len()), &before),
            );
            let mut after = before.clone();
            convert(&mut after, selection, kind);
            let patch = map
                .patch(source, &before, &after)
                .expect("converted block patch");
            let mut updated = source.to_owned();
            updated.replace_range(patch.range, &patch.text);
            assert_eq!(parse(&updated), after, "{source} -> {updated}");
            if source.starts_with("```python") {
                assert!(updated.contains("comment"));
                assert!(
                    after.blocks[0]
                        .text_at(Part::Body)
                        .unwrap()
                        .text
                        .starts_with("# comment")
                );
                assert!(!matches!(
                    parse(&updated).blocks[0].kind,
                    BlockKind::Code { .. }
                ));
            }
        }
    }
}
