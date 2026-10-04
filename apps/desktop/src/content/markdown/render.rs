// Adapted from Bezel 4a7505ab, crates/markdown (MIT). See third_party_licenses/bezel.md.
//! [`Doc`] → gpui elements.
//!
//! Numbers drive layout (sizes, line heights, paddings — the constants here);
//! colors are paint, read from [`Theme`]. Blocks retain their ordered CommonMark
//! container paths; a list inside a quote is distinct from a quote inside a list.
//!
//! Ported from zeronsh/comet (MIT) and rebuilt against the flat block model.

use std::{cell::RefCell, ops::Range, rc::Rc};

use super::inline::{Flat, Offsets, flatten, flatten_with};
use super::style as theme;
use gpui::{
    AnyElement, App, BorderStyle, Bounds, CursorStyle, ElementId, FontWeight, Hsla,
    InteractiveText, ObjectFit, Pixels, Point, SharedString, StyledImage as _, StyledText,
    TextLayout, TextRun, Window, canvas, div, font, img, point, prelude::*, px, quad, size,
};
use gpui_kit as gpui;
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
};
use theme::{TextStyle, Theme, Typeset};

use crate::content::markdown::{
    doc::{Align, Block, BlockKind, Container, Doc, Form, Part, QuoteKind, Text},
    preview,
    select::{Cursor, Selection},
    typography::Typography,
};

/// One indent level. Wide enough to clear a marker and read as a level.
const INDENT_WIDTH: f32 = 22.0;
/// The marker column of a list row.
const MARKER_WIDTH: f32 = 18.0;
const MARKER_GAP: f32 = 8.0;
/// What a fence holds its code in, inside its border.
const CODE_PADDING_X: f32 = 12.0;
const CODE_PADDING_Y: f32 = 10.0;
/// Width of the caret. Wider than a hairline, because it has to read at a
/// glance against the text it sits in.
const CARET_WIDTH: f32 = 1.5;
/// Inline code's wash is a rounded quad painted under the glyphs: a run's
/// `background_color` can only ever be a square box.
const INLINE_CODE_PAD_X: f32 = 0.0;
const INLINE_CODE_INSET_Y: f32 = 1.0;
/// A mention's chip — the same quad-under-glyphs trick as inline code, with
/// more room and an outline so the two do not read as the same thing.
const CHIP_PAD_X: f32 = 4.0;
const CHIP_INSET_Y: f32 = 1.0;
/// A chip with a block to itself is a real element rather than a wash, so it
/// has room for the favicon the inline one cannot hold.
const CHIP_BLOCK_PAD_X: f32 = 8.0;
const CHIP_BLOCK_PAD_Y: f32 = 3.0;
const CHIP_ICON: f32 = 15.0;
/// Bookmark metrics. Notion's card: 180px of image beside the text, and a
/// height that fits a title, two lines of blurb and a footer. A cover moves
/// that image above the text and gives it the card's full width.
const CARD_HEIGHT: f32 = 116.0;
const CARD_IMAGE_WIDTH: f32 = 180.0;
const CARD_COVER_HEIGHT: f32 = 200.0;
const CARD_PADDING: f32 = 14.0;
const CARD_BORDER: f32 = 1.0;
const CARD_ICON: f32 = 16.0;
const CARD_COVER: f32 = 44.0;
/// Image metrics.
const IMAGE_EMPTY_HEIGHT: f32 = 52.0;
const CAPTION_GAP: f32 = 4.0;
/// Table metrics. The design is frameless: hairlines between rows are the only
/// chrome — no outer box, no header fill, no rounding.
const TABLE_CELL_PADDING: f32 = 12.0;
const TABLE_DIVIDER: f32 = 1.0;
/// Floor for a column's max-content share, so a short column ("1k") beside a
/// prose column keeps a readable width.
const TABLE_MIN_COLUMN_CONTENT: f32 = 48.0;
/// Narrowest a column wraps down to before the table scrolls instead.
const TABLE_MIN_COLUMN_WIDTH: f32 = 96.0;

/// Links are delegated to the owning surface so citations and file links can
/// use application navigation instead of opening an external browser.
pub type OnLink = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;
/// Standalone file links may be replaced by the owning surface.
pub type OnFile = Rc<dyn Fn(usize, &str, &str, &App) -> Option<AnyElement>>;
/// Image resources are resolved by the owning surface.
pub type OnImage = Rc<dyn Fn(&str, &App) -> AnyElement>;

/// Source-mapped draft decorations; never part of the document or its history.
#[derive(Clone)]
pub struct Annotation {
    pub at: Cursor,
    pub range: Range<usize>,
    pub icon: Option<SharedString>,
}

/// What an editor paints over a document.
///
/// The caret, selection and layout sink share the same document renderer.
#[derive(Clone)]
pub struct Editing<'a> {
    /// The caret and what it has selected. `None` paints neither — a document
    /// nobody is editing.
    pub selection: Option<Selection>,
    /// The blink's lit half. A caret painted on every frame reads as frozen,
    /// and the phase belongs to whoever owns the focus.
    pub caret_on: bool,
    /// Filled as the document paints, for a caller resolving clicks against it.
    pub layouts: Option<&'a BlockLayouts>,
    /// Shown on the caret's block while it holds nothing.
    pub placeholder: Option<SharedString>,
    /// What to set the document in. `None` derives [`Typography`] from the
    /// application theme; an explicit value can scale the same hierarchy.
    pub typography: Option<Typography>,
    pub on_link: Option<OnLink>,
    pub on_image: Option<OnImage>,
    pub on_file: Option<OnFile>,
    pub annotations: &'a [Annotation],
}

impl Default for Editing<'_> {
    fn default() -> Self {
        Self {
            selection: None,
            // Lit, so that a caller setting a selection and nothing else gets a
            // caret rather than a mystery.
            caret_on: true,
            layouts: None,
            placeholder: None,
            typography: None,
            on_link: None,
            on_image: None,
            on_file: None,
            annotations: &[],
        }
    }
}

/// Where each block's text landed, recorded as it painted.
///
/// A caret has to be placeable by pointer, and only paint knows where a glyph
/// ended up. An editor hands one of these in, the renderer fills it, and the
/// next click resolves against it. Read-only callers pass nothing and pay
/// nothing.
#[derive(Clone, Default)]
pub struct BlockLayouts(Rc<RefCell<Frames>>);

#[derive(Default)]
struct Frames {
    texts: Vec<Painted>,
    rows: Vec<PaintedRow>,
    /// Each block's whole box, which a text layout does not give: a rule and
    /// an image hold no text at all, and a gutter handle still has to find them.
    blocks: Vec<(usize, Bounds<Pixels>)>,
    /// A task block's checkbox, which is not its marker column: the column is
    /// gutter either side of the box, and a click there places a caret.
    checkboxes: Vec<(usize, Bounds<Pixels>)>,
}

/// One shaped run and the slice of its part it covers.
///
/// A paragraph is one entry over all of its text; a code block is one entry per
/// line. The range is what lets both resolve a click the same way — the layout
/// answers in its own coordinates and the base puts the answer back into the
/// part's.
struct Painted {
    block: usize,
    part: Part,
    range: Range<usize>,
    layout: TextLayout,
    offsets: Offsets,
}

struct PaintedRow {
    painted: usize,
    block: usize,
    part: Part,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
}

impl BlockLayouts {
    /// The position under `point`.
    ///
    /// Falls back to the nearest text vertically, so clicking the margin
    /// beside a line — or below the last one — still lands somewhere useful
    /// rather than doing nothing.
    pub fn hit(&self, point: Point<Pixels>) -> Option<Cursor> {
        let frames = self.0.borrow();
        if let Some(row) = frames.rows.iter().find(|row| row.bounds.contains(&point)) {
            return Some(cursor_in_row(&frames, row, point.x));
        }
        frames
            .rows
            .iter()
            .min_by_key(|row| {
                let bounds = row.bounds;
                let above = (bounds.origin.y - point.y).abs();
                let below = (bounds.origin.y + bounds.size.height - point.y).abs();
                f32::from(above.min(below)) as i64
            })
            .map(|row| cursor_in_row(&frames, row, point.x))
    }

    /// Where a position painted last frame, and how tall its line is.
    ///
    /// Vertical motion is geometry rather than arithmetic on line numbers, so
    /// a wrapped row and a hard newline are the same case and neither needs
    /// counting — the rule `ui::TextField` arrived at.
    pub fn position(&self, at: Cursor) -> Option<(Point<Pixels>, Pixels)> {
        let frames = self.0.borrow();
        let painted = frames.texts.iter().find(|painted| {
            painted.block == at.block
                && painted.part == at.part
                && painted.range.start <= at.offset
                && at.offset <= painted.range.end
        })?;
        let point = painted
            .layout
            .position_for_index(painted.offsets.display(at.offset - painted.range.start))?;
        Some((point, painted.layout.line_height()))
    }

    /// The position one painted row above or below `at`, and the row it landed
    /// on. Walks the recorded runs in paint order — which is document order.
    ///
    /// Two things make this refuse to be a hit test. The gap between blocks
    /// belongs to no run, so a probe there answers with whichever run is
    /// nearest — and at a boundary that is the block being *left*, whose bottom
    /// edge is zero pixels away while the next block's top is a whole gap. And
    /// `from` is passed in rather than derived from `at`, because an offset at
    /// a soft wrap belongs to two rows and `position_for_index` always answers
    /// with the first: derive it and every step down recomputes the same row.
    pub fn step_row(
        &self,
        at: Cursor,
        from: Point<Pixels>,
        down: bool,
    ) -> Option<(Cursor, Pixels)> {
        let frames = self.0.borrow();
        let ix = frames
            .rows
            .iter()
            .position(|row| {
                row.block == at.block
                    && row.part == at.part
                    && row_contains(row, at.offset)
                    && row.bounds.origin.y <= from.y
                    && from.y < row.bounds.origin.y + row.bounds.size.height
            })
            .or_else(|| {
                frames.rows.iter().position(|row| {
                    row.block == at.block && row.part == at.part && row_contains(row, at.offset)
                })
            })?;
        let next = match down {
            true => frames.rows.get(ix + 1)?,
            false => frames.rows.get(ix.checked_sub(1)?)?,
        };
        Some((cursor_in_row(&frames, next, from.x), next.bounds.origin.y))
    }

    /// Where a block painted last frame, in window coordinates.
    pub fn block_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .blocks
            .iter()
            .find(|(block, _)| *block == ix)
            .map(|(_, bounds)| *bounds)
    }

    /// Where a task block's checkbox painted, which is the box itself and not
    /// the marker column it sits in — a press outside it is a press on the
    /// gutter, and belongs to whatever handles one.
    pub fn checkbox_bounds(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0
            .borrow()
            .checkboxes
            .iter()
            .find(|(block, _)| *block == ix)
            .map(|(_, bounds)| *bounds)
    }

    fn record(&self, block: usize, part: Part, range: Range<usize>, layout: TextLayout) {
        self.record_mapped(block, part, range, layout, Offsets::default());
    }

    fn record_mapped(
        &self,
        block: usize,
        part: Part,
        range: Range<usize>,
        layout: TextLayout,
        offsets: Offsets,
    ) {
        let mut frames = self.0.borrow_mut();
        let painted = frames.texts.len();
        record_rows(
            &mut frames.rows,
            painted,
            block,
            part,
            &range,
            &layout,
            &offsets,
        );
        frames.texts.push(Painted {
            block,
            part,
            range,
            layout,
            offsets,
        });
    }

    fn record_block(&self, ix: usize, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().blocks.push((ix, bounds));
    }

    fn record_checkbox(&self, ix: usize, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().checkboxes.push((ix, bounds));
    }

    fn clear(&self) {
        let mut frames = self.0.borrow_mut();
        frames.texts.clear();
        frames.rows.clear();
        frames.blocks.clear();

        frames.checkboxes.clear();
    }
}

fn row_contains(row: &PaintedRow, offset: usize) -> bool {
    row.range.start <= offset && offset <= row.range.end
}

fn cursor_in_row(frames: &Frames, row: &PaintedRow, x: Pixels) -> Cursor {
    let painted = &frames.texts[row.painted];
    let y = row.bounds.origin.y + row.bounds.size.height / 2.0;
    let (Ok(offset) | Err(offset)) = painted.layout.index_for_position(point(x, y));
    Cursor::new(
        painted.block,
        painted.part,
        painted.range.start + painted.offsets.source(offset).min(painted.range.len()),
    )
}

fn record_rows(
    rows: &mut Vec<PaintedRow>,
    painted: usize,
    block: usize,
    part: Part,
    range: &Range<usize>,
    layout: &TextLayout,
    offsets: &Offsets,
) {
    let line_height = layout.line_height();
    let bounds = layout.bounds();
    let mut origin = bounds.origin;
    let mut line_start = 0;
    for line in layout.line_layouts() {
        let shaped = &line.unwrapped_layout;
        let row_ends = line
            .wrap_boundaries()
            .iter()
            .map(|wrap| shaped.runs[wrap.run_ix].glyphs[wrap.glyph_ix].index)
            .chain([line.len()]);
        let mut row_start = 0;
        for (row, row_end) in row_ends.enumerate() {
            rows.push(PaintedRow {
                painted,
                block,
                part,
                range: range.start + offsets.source(line_start + row_start)
                    ..range.start + offsets.source(line_start + row_end),
                bounds: Bounds::new(
                    origin + point(px(0.0), line_height * row as f32),
                    size(bounds.size.width, line_height),
                ),
            });
            row_start = row_end;
        }
        origin.y += line.size(line_height).height;
        line_start += line.len() + 1;
    }
}

/// What the editor needs painted into one text: which text it is, where the
/// caret sits, and where to record the layout a click resolves against.
///
/// One bundle rather than four parameters threaded through every block arm —
/// a read-only render builds it with no caret and no sink, and pays nothing.
#[derive(Clone, Copy)]
struct Overlay<'a> {
    block: usize,
    part: Part,
    selection: Option<Selection>,
    caret_on: bool,
    layouts: Option<&'a BlockLayouts>,
    /// Shown on the caret's block while it holds nothing. The renderer is the
    /// only thing that knows where that text sits, so the string comes to it.
    placeholder: Option<&'a SharedString>,
    on_link: Option<&'a OnLink>,
    on_image: Option<&'a OnImage>,
    on_file: Option<&'a OnFile>,
    annotations: &'a [Annotation],
}

impl<'a> Overlay<'a> {
    fn at(self, part: Part) -> Self {
        Self { part, ..self }
    }

    fn here(&self) -> Cursor {
        Cursor::new(self.block, self.part, 0)
    }

    /// The caret to paint: where it is, and only on the blink's lit half.
    ///
    /// Separate from [`Self::caret`] because the blink must not reach anything
    /// but the quad — a block whose paint depends on holding the caret would
    /// otherwise swap itself out twice a second.
    fn caret_painted(&self) -> Option<usize> {
        self.caret_on.then(|| self.caret()).flatten()
    }

    /// The caret's byte offset, if the head is in *this* text.
    fn caret(&self) -> Option<usize> {
        self.selection
            .map(|selection| selection.head)
            .filter(|head| head.block == self.block && head.part == self.part)
            .map(|head| head.offset)
    }

    /// The selected slice of this text, clipped to it.
    fn selected(&self, len: usize) -> Option<Range<usize>> {
        self.clip(self.selection?, len)
    }

    /// A range clipped to this text, and `None` when it does not reach it.
    ///
    /// The comparison is on `(block, part)` alone: a range covers this text
    /// entirely when it starts before and ends after, and the offsets only
    /// matter at the two ends.
    fn clip(&self, selection: Selection, len: usize) -> Option<Range<usize>> {
        if selection.is_collapsed() {
            return None;
        }
        let (start, end) = selection.ordered();
        let here = self.here();
        let (first, last) = (
            Cursor::new(start.block, start.part, 0),
            Cursor::new(end.block, end.part, 0),
        );
        if here < first || here > last {
            return None;
        }
        let from = if here == first { start.offset } else { 0 };
        let to = if here == last { end.offset } else { len };
        (from < to).then_some(from..to.min(len))
    }

    /// Whether a block painting something a caret cannot enter — a rule, a
    /// picture — falls inside the selection, and so should show that it is
    /// going to be taken.
    fn covers_block(&self) -> bool {
        let Some(selection) = self.selection.filter(|s| !s.is_collapsed()) else {
            return false;
        };
        let (start, end) = selection.ordered();
        start.block < self.block && self.block < end.block
    }
}

/// Render a document with a caret and a selection in it.
///
/// Both are paint-time concerns and nothing else: they read their positions off
/// the shaped text's own layout handle, the same way the inline-code wash does,
/// so nothing about layout depends on where the caret sits. An editor supplies
/// the selection and owns the focus and the keys; painting a caret and a few
/// quads is not worth a second renderer.
pub fn render_with(doc: &Doc, editing: Editing, window: &mut Window, cx: &mut App) -> AnyElement {
    render_with_block(doc, editing, None, window, cx)
}

/// Replace only a focused leaf with a framework input while retaining the
/// surrounding Bezel layout and CommonMark container path.
pub fn render_with_block(
    doc: &Doc,
    editing: Editing,
    mut replacement: Option<(usize, AnyElement)>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Editing {
        selection,
        caret_on,
        layouts,
        placeholder,
        typography,
        on_link,
        on_image,
        on_file,
        annotations,
    } = editing;
    // Refilled every frame, in paint order — and emptied in *prepaint*, not
    // here. An editor reads last frame's positions while building this frame's
    // tree (a menu anchored at the caret, a handle beside a block), and
    // clearing at build time takes them away before it can. Placed first in the
    // column so it runs ahead of every recorder below it.
    let reset = layouts.map(|layouts| {
        let layouts = layouts.clone();
        canvas(move |_, _, _| layouts.clear(), |_, _, _, _| ())
            .absolute()
            .size(px(0.0))
    });
    // Cloned once so the theme is readable while `cx` stays free for the
    // element state the copy button needs.
    let theme = Theme::of(cx).clone();
    let typography = typography.unwrap_or_else(|| Typography::of(cx));
    let mut column = div()
        .flex()
        .flex_col()
        .min_w_0()
        .w_full()
        .font_family(theme.font_body.clone())
        .text_color(theme.text)
        .children(reset);

    let spacing = cx.theme().spacing_tokens();
    for (ix, block) in doc.blocks.iter().enumerate() {
        let gap = match doc.blocks.get(ix.wrapping_sub(1)) {
            None => px(0.),
            Some(previous) if tight(previous, block) => spacing.sm,
            Some(_) => spacing.lg,
        };
        let overlay = Overlay {
            block: ix,
            part: Part::Body,
            selection,
            caret_on,
            layouts,
            placeholder: placeholder.as_ref(),
            on_link: on_link.as_ref(),
            on_image: on_image.as_ref(),
            on_file: on_file.as_ref(),
            annotations,
        };
        // The block's own box, recorded for a gutter handle and a drop target.
        // A rule and an image hold no text, so a layout would not find them.
        let frame = layouts.map(|layouts| {
            let layouts = layouts.clone();
            canvas(
                move |bounds, _, _| layouts.record_block(ix, bounds),
                |_, _, _, _| (),
            )
            .absolute()
            .size_full()
        });
        let leaf = if replacement.as_ref().is_some_and(|(block, _)| *block == ix) {
            replacement.take().unwrap().1
        } else {
            block_element(block, overlay, &typography, &theme, window, cx)
        };
        let mut element = div()
            .debug_selector(move || format!("markdown-editor-block-{ix}"))
            .w_full()
            .relative()
            .children(frame)
            .when(overlay.covers_block() && block.opaque(), |el| {
                el.rounded(px(theme.radius)).bg(theme.selection)
            })
            .child(leaf)
            .into_any_element();
        let mut marker = list_marker(block, overlay, &typography, &theme);
        for container in block.containers.iter().rev() {
            element = match container {
                Container::List if marker.is_some() => div()
                    .flex()
                    .items_start()
                    .gap(px(MARKER_GAP))
                    .child(marker.take().unwrap())
                    .child(div().flex_1().min_w_0().child(element))
                    .into_any_element(),
                Container::List => div().pl(px(INDENT_WIDTH)).child(element).into_any_element(),
                Container::Quote(kind) => div()
                    .border_l_2()
                    .border_color(
                        kind.map_or(theme.border_strong, |kind| alert_color(kind, &theme)),
                    )
                    .pl(px(12.))
                    .pr(px(10.))
                    .py(px(2.))
                    .text_color(theme.text_muted)
                    .children(
                        kind.filter(|_| {
                            doc.blocks
                                .get(ix.wrapping_sub(1))
                                .is_none_or(|previous| previous.containers != block.containers)
                        })
                        .map(|kind| {
                            div()
                                .pb(px(2.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(alert_color(kind, &theme))
                                .child(alert_label(kind))
                        }),
                    )
                    .child(element)
                    .into_any_element(),
            };
        }
        column = column.child(
            div()
                .mt(gap)
                .when(block.containers.is_empty(), |el| {
                    el.pl(px(block.indent as f32 * INDENT_WIDTH))
                })
                .child(element),
        );
    }

    column.into_any_element()
}

/// Whether two adjacent blocks belong to the same list and should sit close.
fn tight(previous: &Block, next: &Block) -> bool {
    let marker = |block: &Block| {
        matches!(
            block.kind,
            BlockKind::Bullet(_) | BlockKind::Ordered { .. } | BlockKind::Task { .. }
        )
    };
    marker(previous) && (marker(next) || next.indent > previous.indent)
}

fn list_marker(
    block: &Block,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
) -> Option<AnyElement> {
    match &block.kind {
        BlockKind::Bullet(_) => Some(disc(overlay.block, typography, theme)),
        BlockKind::Ordered { number, .. } => {
            Some(number_marker(*number, overlay.block, typography, theme))
        }
        BlockKind::Task { checked, .. } => Some(checkbox(*checked, overlay, typography, theme)),
        _ => None,
    }
}

fn block_element(
    block: &Block,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if let Some(render) = overlay.on_file
        && let Some((url, label)) = super::files::reference(block)
        && let Some(card) = render(overlay.block, url, label, cx)
    {
        return card;
    }
    let body = overlay.at(Part::Body);
    match &block.kind {
        BlockKind::Bullet(text)
        | BlockKind::Ordered { text, .. }
        | BlockKind::Task { text, .. }
            if !block.containers.is_empty() =>
        {
            text_element(
                text,
                typography.body.size(),
                typography.body.line_height(),
                FontWeight::NORMAL,
                body,
                theme,
                cx,
            )
        }
        BlockKind::Paragraph(text) => text_element(
            text,
            typography.body.size(),
            typography.body.line_height(),
            FontWeight::NORMAL,
            body,
            theme,
            cx,
        ),
        BlockKind::Heading { level, text } => {
            let heading = typography.heading(*level);
            text_element(
                text,
                heading.size(),
                heading.line_height(),
                heading.weight,
                body,
                theme,
                cx,
            )
        }
        BlockKind::Bullet(text) => marker_row(
            disc(overlay.block, typography, theme),
            text,
            body,
            typography,
            theme,
            cx,
        ),
        BlockKind::Ordered { number, text } => marker_row(
            number_marker(*number, overlay.block, typography, theme),
            text,
            body,
            typography,
            theme,
            cx,
        ),
        BlockKind::Task { checked, text } => marker_row(
            checkbox(*checked, overlay, typography, theme),
            text,
            body,
            typography,
            theme,
            cx,
        ),
        BlockKind::Quote { kind, text } => div()
            .border_l_2()
            .border_color(kind.map_or(theme.border_strong, |kind| alert_color(kind, theme)))
            .pl(px(12.0))
            .pr(px(10.0))
            .py(px(2.0))
            .text_color(theme.text_muted)
            .children(kind.map(|kind| {
                div()
                    .pb(px(2.0))
                    .text_size(px(typography.body.size()))
                    .line_height(px(typography.body.line_height()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(alert_color(kind, theme))
                    .child(alert_label(kind))
            }))
            .child(text_element(
                text,
                typography.body.size(),
                typography.body.line_height(),
                FontWeight::NORMAL,
                body,
                theme,
                cx,
            ))
            .into_any_element(),
        BlockKind::Code { language, code } => code_block(
            language.as_deref(),
            &code.text,
            overlay.at(Part::Code),
            typography,
            theme,
            window,
            cx,
        ),
        BlockKind::Image { url, alt, width } => {
            image(url, alt, *width, overlay, typography, theme, cx)
        }
        BlockKind::Bookmark { url, form } => bookmark(
            overlay.block,
            url,
            *form,
            overlay.on_link.cloned(),
            typography,
            theme,
            cx,
        ),
        BlockKind::Table {
            align,
            header,
            rows,
        } => table(align, header, rows, overlay, typography, theme, window, cx),
        BlockKind::Rule => div()
            .h(px(1.0))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
    }
}

fn number_marker(number: u64, block: usize, typography: &Typography, theme: &Theme) -> AnyElement {
    div()
        .debug_selector(move || format!("markdown-marker-{block}"))
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(typography.body.line_height()))
        .text_size(px(typography.body.size()))
        .line_height(px(typography.body.line_height()))
        .text_center()
        .text_color(theme.text_muted)
        .child(SharedString::from(format!("{number}.")))
        .into_any_element()
}

/// A real 5px disc rather than the "•" glyph, which reads too small at body size.
fn disc(block: usize, typography: &Typography, theme: &Theme) -> AnyElement {
    div()
        .debug_selector(move || format!("markdown-marker-{block}"))
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(typography.body.line_height()))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .debug_selector(move || format!("markdown-bullet-{block}"))
                .w(px(5.0))
                .h(px(5.0))
                .rounded_full()
                .bg(theme.text_faint),
        )
        .into_any_element()
}

fn checkbox(checked: bool, overlay: Overlay, typography: &Typography, theme: &Theme) -> AnyElement {
    let ix = overlay.block;
    let mut box_ = div()
        .relative()
        .w(px(13.0))
        .h(px(13.0))
        .rounded(px(theme.radius * 0.6))
        .border_1()
        .flex()
        .items_center()
        .justify_center();
    box_ = if checked {
        box_.bg(theme.solid)
            .border_color(theme.solid)
            .text_style(TextStyle::Caption, theme)
            .text_color(theme.on_solid)
            .child("✓")
    } else {
        box_.border_color(theme.border_strong)
    };
    // The box's own bounds rather than the marker column's: a caller hit-tests
    // these to tell a toggle from a caret placed in the gutter beside it.
    box_ = box_.children(overlay.layouts.map(|layouts| {
        let layouts = layouts.clone();
        canvas(
            move |bounds, _, _| layouts.record_checkbox(ix, bounds),
            |_, _, _, _| (),
        )
        .absolute()
        .size_full()
    }));

    div()
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(typography.body.line_height()))
        .flex()
        .items_center()
        .child(box_)
        .into_any_element()
}

/// What an alert paints its rule and its label in.
fn alert_label(kind: QuoteKind) -> SharedString {
    crate::tr(match kind {
        QuoteKind::Note => "content_note",
        QuoteKind::Tip => "content_tip",
        QuoteKind::Important => "content_important",
        QuoteKind::Warning => "content_warning",
        QuoteKind::Caution => "content_caution",
    })
}

fn alert_color(kind: QuoteKind, theme: &Theme) -> Hsla {
    match kind {
        QuoteKind::Note => theme.accent,
        QuoteKind::Tip => theme.success,
        QuoteKind::Important => theme.busy,
        QuoteKind::Warning => theme.warning,
        QuoteKind::Caution => theme.danger,
    }
}

fn marker_row(
    marker: AnyElement,
    text: &Text,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .gap(px(MARKER_GAP))
        .child(marker)
        .child(div().flex_1().min_w_0().child(text_element(
            text,
            typography.body.size(),
            typography.body.line_height(),
            FontWeight::NORMAL,
            overlay,
            theme,
            cx,
        )))
        .into_any_element()
}

fn text_element(
    text: &Text,
    size: f32,
    line_height: f32,
    weight: FontWeight,
    overlay: Overlay,
    theme: &Theme,
    _cx: &App,
) -> AnyElement {
    let annotations = overlay
        .annotations
        .iter()
        .filter(|annotation| annotation.at == overlay.here())
        .cloned()
        .collect::<Vec<_>>();
    let flat = flatten_with(text, weight, theme, &annotations);
    painted_text(flat, text.text.len(), size, line_height, overlay, theme)
}

/// Shaped inline content with the editing overlay under it: the selection, the
/// caret, the inline-code wash, and the layout a click resolves against.
///
/// Takes a [`Flat`] rather than a [`Text`] because a table has to shape every
/// cell to measure the columns before it can paint one.
fn painted_text(
    flat: Flat,
    len: usize,
    size: f32,
    line_height: f32,
    overlay: Overlay,
    theme: &Theme,
) -> AnyElement {
    let (ix, part) = (overlay.block, overlay.part);
    let (caret, selected) = (overlay.caret_painted(), overlay.selected(len));
    let span = 0..len;
    // Only where the caret already is, and only while there is nothing to
    // read: a hint on every empty block would be a page of grey.
    let hint = overlay
        .placeholder
        // The caret's own presence, not the blink's phase — a hint that came
        // and went twice a second would be unreadable.
        .filter(|_| len == 0 && overlay.caret().is_some())
        .map(|hint| {
            div()
                .absolute()
                .text_color(theme.text_faint)
                .child(hint.clone())
        });
    let offsets = flat.offsets;
    let selected = selected.map(|range| offsets.display(range.start)..offsets.display(range.end));
    let caret = caret.map(|offset| offsets.display(offset));
    let styled = StyledText::new(flat.text).with_runs(flat.runs);
    let layout = styled.layout().clone();

    let painted: AnyElement = if flat.links.is_empty() {
        styled.into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = flat.links.into_iter().unzip();
        let on_link = overlay.on_link.cloned();
        InteractiveText::new(SharedString::from(format!("md-text-{ix}-{part:?}")), styled)
            .on_click(ranges, move |clicked, window, cx| {
                if let Some(url) = urls.get(clicked) {
                    if let Some(on_link) = &on_link {
                        on_link(&SharedString::from(url.clone()), window, cx);
                    } else {
                        cx.open_url(url);
                    }
                }
            })
            .into_any_element()
    };

    // The wash is painted before the text — an earlier sibling is underneath —
    // reading glyph geometry from the text's own layout handle. Pure paint,
    // never part of layout.
    let wash = theme.code_wash;
    let code_ranges = flat.code;
    let chip_wash = theme.element_hover;
    let chip_edge = theme.border;
    let chip_ranges = flat.chips;
    let caret_color = theme.caret;
    let selection_color = theme.selection;
    let layouts = overlay.layouts.cloned();
    let radius = theme.radius;
    use gpui_kit::component::{IconName, IconNamed};
    let icons: Vec<_> = flat
        .icons
        .into_iter()
        .map(|range| (range, IconName::ExternalLink.path()))
        .chain(flat.references)
        .collect();
    let link_color = theme.link;
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            if let Some(layouts) = &layouts {
                layouts.record_mapped(ix, part, span.clone(), layout.clone(), offsets.clone());
            }
            // Under the glyphs, like the inline-code wash — one quad per visual
            // row, so a wrapped selection is a stack of rows rather than a box
            // around all of them.
            if let Some(range) = &selected {
                for rect in range_rects(&layout, range, 0.0, 0.0) {
                    window.paint_quad(quad(
                        rect,
                        px(2.0),
                        selection_color,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            if let Some(offset) = caret
                && let Some(head) = layout.position_for_index(offset)
            {
                window.paint_quad(quad(
                    caret_quad(head, size, layout.line_height()),
                    px(0.0),
                    caret_color,
                    px(0.0),
                    gpui::transparent_black(),
                    BorderStyle::default(),
                ));
            }
            for range in &code_ranges {
                for rect in range_rects(&layout, range, INLINE_CODE_PAD_X, INLINE_CODE_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        px(radius),
                        wash,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            // Wider, rounder and outlined, so a chip and an inline code span
            // never read as the same thing at a glance.
            for range in &chip_ranges {
                for rect in range_rects(&layout, range, CHIP_PAD_X, CHIP_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        px(radius),
                        chip_wash,
                        px(1.0),
                        chip_edge,
                        BorderStyle::Solid,
                    ));
                }
            }
            for (icon, path) in &icons {
                if let Some(rect) = range_rects(&layout, icon, 0., 0.).first() {
                    let side = px(size * 0.85);
                    let bounds = Bounds::new(
                        rect.origin + point(px(0.), (rect.size.height - side) / 2.),
                        gpui::size(side, side),
                    );
                    let _ = window.paint_svg(
                        bounds,
                        path.clone(),
                        None,
                        gpui::TransformationMatrix::default(),
                        link_color,
                        cx,
                    );
                }
            }
        },
    )
    .absolute()
    .size_full();

    div()
        .text_size(px(size))
        .line_height(px(line_height))
        .relative()
        .child(underlay)
        .children(hint)
        .child(painted)
        .into_any_element()
}

/// The caret's quad: the text's own size, centred in the line box.
///
/// The leading is not the caret's to take. A document is set with air around
/// its lines, and a caret filling all of it reads as a second, larger font
/// standing where the text should be.
fn caret_quad(head: Point<Pixels>, size: f32, line_height: Pixels) -> Bounds<Pixels> {
    let inset = (line_height - px(size)) / 2.0;
    Bounds::new(
        head + point(px(0.0), inset),
        gpui::size(px(CARET_WIDTH), px(size)),
    )
}

/// The rectangles a byte range occupies, one per visual row.
fn range_rects(
    layout: &gpui::TextLayout,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
) -> Vec<Bounds<Pixels>> {
    let mut rects = Vec::new();
    let line_height = layout.line_height();
    let mut origin = layout.bounds().origin;
    let mut line_start = 0;
    for line in layout.line_layouts() {
        let shaped = &line.unwrapped_layout;
        // A wrap boundary index is both the end of one row and the start of
        // the next.
        let row_ends = line
            .wrap_boundaries()
            .iter()
            .map(|wrap| shaped.runs[wrap.run_ix].glyphs[wrap.glyph_ix].index)
            .chain([line.len()]);
        let mut row_start = 0;
        for (row, row_end) in row_ends.enumerate() {
            let from = range
                .start
                .saturating_sub(line_start)
                .clamp(row_start, row_end);
            let to = range.end.saturating_sub(line_start).min(row_end);
            let row_x = shaped.x_for_index(row_start);
            let (left, right) = (shaped.x_for_index(from), shaped.x_for_index(to));
            if from < to && right > left {
                rects.push(Bounds::new(
                    origin
                        + point(
                            left - row_x - px(pad_x),
                            line_height * row as f32 + px(inset_y),
                        ),
                    size(
                        right - left + px(2.0 * pad_x),
                        line_height - px(2.0 * inset_y),
                    ),
                ));
            }
            row_start = row_end;
        }
        origin.y += line.size(line_height).height;
        // The newline between two lines is a byte of the text and of neither.
        line_start += line.len() + 1;
    }
    rects
}

/// The shaped lines of a fence, and the canvas that paints the caret, the
/// selection over them.
fn code_lines(
    language: Option<&str>,
    code: &str,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    cx: &App,
) -> (AnyElement, Vec<AnyElement>) {
    let ix = overlay.block;
    // Reuse the same Kit syntax backend as the source editor and diff view.
    // Unknown languages keep the code block's plain-text presentation.
    let spans = language.map(|language| crate::content::syntax::highlight(language, code, cx));
    let mono = font(theme.font_mono.clone());
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: mono.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    // Each source line's own layout, with the slice of the code it covers —
    // the caret and a click both resolve through these. A wrapped line is
    // several rows of one layout, which is the case `range_rects` already
    // walks for a paragraph.
    let mut rows: Vec<(Range<usize>, TextLayout)> = Vec::new();
    let mut offset = 0usize;
    let lines: Vec<AnyElement> = code
        .split('\n')
        .map(|line| {
            let start = offset;
            offset += line.len() + 1;
            let mut runs = Vec::new();
            // Runs are measured within the line; spans are byte ranges over the
            // whole block, so every span is clipped to the line and rebased.
            let mut pos = 0usize;
            if let Some(spans) = &spans {
                let end = start + line.len();
                for (range, kind) in spans.iter().filter(|(r, _)| r.end > start && r.start < end) {
                    let s = range.start.clamp(start, end) - start;
                    let e = range.end.min(end) - start;
                    if s > pos {
                        runs.push(run(s - pos, theme.text));
                    }
                    runs.push(run(e - s, kind.color.unwrap_or(theme.text)));
                    pos = e;
                }
            }
            if pos < line.len() {
                runs.push(run(line.len() - pos, theme.text));
            }
            if runs.is_empty() {
                runs.push(run(0, theme.text));
            }
            let styled = StyledText::new(SharedString::from(line.to_string())).with_runs(runs);
            rows.push((start..start + line.len(), styled.layout().clone()));
            styled.into_any_element()
        })
        .collect();

    let caret = overlay.caret_painted();
    let selected = overlay.selected(code.len());
    let sink = overlay.layouts.cloned();
    let code_size = typography.code.size();
    let (caret_color, selection_color) = (theme.caret, theme.selection);
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            for (span, layout) in &rows {
                if let Some(sink) = &sink {
                    sink.record(ix, Part::Code, span.clone(), layout.clone());
                }
                if let Some(range) = &selected {
                    let (from, to) = (range.start.max(span.start), range.end.min(span.end));
                    if from < to {
                        for rect in
                            range_rects(layout, &(from - span.start..to - span.start), 0.0, 0.0)
                        {
                            window.paint_quad(quad(
                                rect,
                                px(2.0),
                                selection_color,
                                px(0.0),
                                gpui::transparent_black(),
                                BorderStyle::default(),
                            ));
                        }
                    }
                }
                if let Some(offset) = caret.filter(|at| span.contains(at) || *at == span.end)
                    && let Some(head) = layout.position_for_index(offset - span.start)
                {
                    window.paint_quad(quad(
                        caret_quad(head, code_size, layout.line_height()),
                        px(0.0),
                        caret_color,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
        },
    )
    .absolute()
    .size_full();

    (underlay.into_any_element(), lines)
}

fn code_block(
    language: Option<&str>,
    code: &str,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let ix = overlay.block;
    let (underlay, lines) = code_lines(language, code, overlay, typography, theme, cx);
    let body = code_body(ix, underlay, lines, typography, true);

    div()
        .rounded(px(theme.radius))
        .bg(theme.ink(0.035))
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .relative()
        // Keep the copy button visible even when a fence has no language tag.
        .child(
            div()
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .px(px(CODE_PADDING_X))
                .py(px(5.0))
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.ink(0.02))
                .text_style(TextStyle::Subheadline, theme)
                .text_color(match language {
                    Some(_) => theme.text_muted,
                    None => theme.text_faint,
                })
                .child(
                    div().relative().child(SharedString::from(
                        language
                            .map(str::to_owned)
                            .unwrap_or_else(|| crate::tr("content_plain").to_string()),
                    )),
                ),
        )
        .child(body)
        .child(copy_button(code, ix, theme, window, cx))
        .into_any_element()
}

/// The lines of a fence, wrapped to the block or scrolling sideways under it.
fn code_body(
    ix: usize,
    underlay: AnyElement,
    lines: Vec<AnyElement>,
    typography: &Typography,
    wrap: bool,
) -> AnyElement {
    let column = div()
        .flex()
        .flex_col()
        .px(px(CODE_PADDING_X))
        .children(lines);
    let body = div()
        .id(ElementId::named_usize("md-code", ix))
        .relative()
        .py(px(CODE_PADDING_Y))
        .text_size(px(typography.code.size()))
        .line_height(px(typography.code.line_height()))
        .child(underlay);
    if wrap {
        // The column is the block's width here rather than its widest line's,
        // which is what gives the text something to wrap against.
        body.child(column.w_full()).into_any_element()
    } else {
        super::scroll::horizontal(
            format!("md-code-scroll-{ix}"),
            body.flex()
                .flex_row()
                .whitespace_nowrap()
                // The padding belongs to the lines, not to the scroller: a scroll
                // container's trailing padding is not part of what it will scroll
                // to, so the last characters of a long line sit behind the right
                // edge with nowhere left to go. As a row's only item this column is
                // sized by its widest line, and the padding rides along inside that
                // width.
                .child(column.items_start()),
        )
        .into_any_element()
    }
}

/// A copy button that owns its own feedback.
///
/// The state is the element's, not the caller's: a component library cannot ask
/// every host to thread a handler and a "which block is showing Copied" index
/// through its render tree just to put a button on a code block. It resets when
/// the pointer leaves, which needs no clock.
fn copy_button(
    code: &str,
    ix: usize,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let copied = window.use_keyed_state(ElementId::named_usize("md-copied", ix), cx, |_, _| false);
    let showing = *copied.read(cx);
    let text: SharedString = code.to_string().into();

    Button::new(ElementId::named_usize("md-copy", ix))
        .ghost()
        .small()
        .compact()
        .absolute()
        .top(px(3.0))
        .right(px(5.0))
        .h(px(20.0))
        .px(px(6.0))
        .rounded(px(theme.radius))
        .text_style(TextStyle::Caption, theme)
        .text_color(theme.text_muted)
        .label(crate::tr(if showing {
            "content_copied"
        } else {
            "content_copy"
        }))
        .on_click({
            let copied = copied.clone();
            move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
                copied.update(cx, |state, cx| {
                    *state = true;
                    cx.notify();
                });
            }
        })
        .on_hover(move |hovering, _, cx| {
            if !*hovering && *copied.read(cx) {
                copied.update(cx, |state, cx| {
                    *state = false;
                    cx.notify();
                });
            }
        })
        .into_any_element()
}

/// A picture and the caption under it, which is the alt text a caret can reach.
///
/// The caption row appears when there is something to read or somewhere to
/// type, so a document being read is not a column of pictures each trailing a
/// blank line. With no URL yet the picture is a dashed row instead — the shape
/// the slash menu makes, waiting to be told what to show.
fn image(
    url: &str,
    alt: &Text,
    width: Option<u32>,
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    cx: &App,
) -> AnyElement {
    let hint = crate::tr("content_caption");
    let overlay = Overlay {
        placeholder: Some(&hint),
        ..overlay.at(Part::Caption)
    };
    let picture = if let Some(render) = overlay.on_image {
        div().child(render(url, cx))
    } else if url.is_empty() {
        div()
            .h(px(IMAGE_EMPTY_HEIGHT))
            .flex()
            .items_center()
            .px(px(CARD_PADDING))
            .rounded(px(theme.radius))
            .border_1()
            .border_dashed()
            .border_color(theme.border)
            .text_size(px(typography.body.size()))
            .text_color(theme.text_muted)
            .child(crate::tr("content_add_image"))
    } else {
        // A URL is fetched; anything else is a file, and gpui reads one only
        // from a `PathBuf` — handed a string it looks for an asset built into
        // the binary and paints nothing.
        let picture = match url.contains("://") {
            true => img(SharedString::from(url.to_string())),
            false => img(std::path::PathBuf::from(url)),
        };
        let box_ = div()
            .relative()
            .rounded(px(theme.radius))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border);
        match width {
            // A stated width is the box's: it hugs, so the border is around
            // the picture rather than around the column beside it, and the
            // picture fills what the box settled on — which `max_w_full`
            // holds inside the page however wide the width was written.
            Some(width) => box_
                .self_start()
                .max_w_full()
                .w(px(width as f32))
                .child(picture.w(px(width as f32)).max_w_full()),
            // Unstated, the picture scales itself against the column, which
            // is a percentage and so needs a box that spans one to measure.
            None => box_.child(picture.max_w_full()),
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(CAPTION_GAP))
        .child(picture)
        // An empty caption still paints while the caret is in it, or there
        // would be nothing to type into and no hint saying so.
        .when(!alt.is_empty() || overlay.caret().is_some(), |el| {
            el.child(text_element(
                alt,
                typography.caption.size(),
                typography.caption.line_height(),
                FontWeight::NORMAL,
                overlay,
                theme,
                cx,
            ))
        })
        .into_any_element()
}

/// A bookmark, in Notion's proportions: a fixed-height row with the text on the
/// left and an image panel of a fixed width on the right, all of it one click
/// target. [`Form::Embed`] turns the row into a column and gives the image the
/// card's full width instead, and [`Form::Chip`] is neither — a pill of favicon
/// and title, which is what an inline mention would be if shaped text had
/// anywhere to put a picture.
///
/// The row keeps Bezel's fixed height and pinned footer. This adapter paints
/// URL metadata only and does not fetch a remote preview.
fn bookmark(
    ix: usize,
    url: &str,
    form: Form,
    on_link: Option<OnLink>,
    typography: &Typography,
    theme: &Theme,
    _cx: &App,
) -> AnyElement {
    let preview = preview::Preview::default();
    let host = SharedString::from(preview::host(url).to_string());
    let label = preview.label.clone().unwrap_or_else(|| host.clone());
    let title = preview
        .title
        .clone()
        .unwrap_or_else(|| SharedString::from(url.to_string()));

    // Owned, because the image panel's fallback outlives this call: gpui asks
    // for the replacement element only once the fetch has failed.
    let (icon, muted, wash) = (preview.icon.clone(), theme.text_muted, theme.element_hover);
    let site = host.clone();
    let mark = move |size: f32| {
        let host = site.clone();
        match icon.clone() {
            Some(icon) => img(icon)
                .size(px(size))
                .rounded(px(size / 4.0))
                .with_fallback(move || initial(&host, size, muted, wash))
                .into_any_element(),
            None => initial(&host, size, muted, wash),
        }
    };

    if form == Form::Chip {
        let open = url.to_string();
        let pill = div()
            .id(ElementId::named_usize("md-chip", ix))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px(px(CHIP_BLOCK_PAD_X))
            .py(px(CHIP_BLOCK_PAD_Y))
            .rounded(px(theme.radius))
            .border_1()
            .border_color(theme.border)
            .bg(theme.element_hover)
            .text_size(px(typography.body.size()))
            .line_height(px(typography.body.line_height()))
            .text_color(theme.text)
            .cursor(CursorStyle::PointingHand)
            .hover(|el| el.bg(theme.element_active))
            .on_click(move |_, window, cx| {
                if let Some(on_link) = &on_link {
                    on_link(&open.clone().into(), window, cx);
                } else {
                    cx.open_url(&open);
                }
            })
            .child(mark(CHIP_ICON))
            // The host, not the URL, when nothing has resolved it: a chip is
            // the short form, and a raw URL in a pill is the long one.
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .child(preview.title.unwrap_or(label)),
            );
        // A block's own box is `display: block`, where a pill would take the
        // whole width. One flex row around it is what lets it hug its label.
        return div().flex().flex_row().child(pill).into_any_element();
    }

    let words = div()
        .flex()
        .flex_col()
        .min_w_0()
        .px(px(CARD_PADDING))
        .py(px(CARD_PADDING - 2.0))
        .child(
            div()
                .truncate()
                .text_size(px(typography.body.size()))
                .line_height(px(typography.body.line_height()))
                .text_color(theme.text)
                .child(title),
        )
        .children(preview.description.map(|blurb| {
            div()
                .line_clamp(2)
                .text_size(px(typography.card.size()))
                .line_height(px(typography.card.line_height()))
                .text_color(theme.text_muted)
                .child(blurb)
        }))
        .child(
            div()
                .mt_auto()
                .pt(px(6.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(typography.card.size()))
                .text_color(theme.text_muted)
                .child(mark(CARD_ICON))
                .child(div().truncate().child(label)),
        );

    let picture = corners(div(), form, theme.radius)
        .bg(theme.surface)
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .child(match preview.image {
            Some(image) => corners(
                img(image).size_full().object_fit(ObjectFit::Cover),
                form,
                theme.radius,
            )
            .with_fallback(move || mark(CARD_COVER))
            .into_any_element(),
            None => mark(CARD_COVER),
        });

    let open = url.to_string();
    let card = div()
        .id(ElementId::named_usize("md-bookmark", ix))
        .flex()
        .w_full()
        .overflow_hidden()
        .rounded(px(theme.radius))
        .border(px(CARD_BORDER))
        .border_color(theme.border)
        .bg(theme.surface_card)
        .cursor(CursorStyle::PointingHand)
        .hover(|el| el.bg(theme.element_hover))
        .on_click(move |_, window, cx| {
            if let Some(on_link) = &on_link {
                on_link(&open.clone().into(), window, cx);
            } else {
                cx.open_url(&open);
            }
        });

    if form == Form::Embed {
        card.flex_col()
            .child(picture.w_full().h(px(CARD_COVER_HEIGHT)))
            .child(words.w_full())
    } else {
        card.h(px(CARD_HEIGHT))
            .child(words.flex_1())
            .child(picture.flex_none().w(px(CARD_IMAGE_WIDTH)).h_full())
    }
    .into_any_element()
}

/// The card's corners, on the panel that reaches them: a content mask is a
/// rectangle, so a picture paints square over a rounded card unless it carries
/// the radius itself, concentric inside the card's border.
fn corners<T: Styled>(element: T, form: Form, radius: f32) -> T {
    let corner = px((radius - CARD_BORDER).max(0.));
    match form {
        Form::Embed => element.rounded_t(corner),
        _ => element.rounded_r(corner),
    }
}

/// The mark a site gets before anyone has fetched its favicon: its host's first
/// letter, which is a placeholder no icon set has to ship.
fn initial(host: &str, size: f32, color: Hsla, wash: Hsla) -> AnyElement {
    div()
        .flex_none()
        .size(px(size))
        .rounded(px(size / 4.0))
        .bg(wash)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.55))
        .text_color(color)
        .child(SharedString::from(
            host.chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string(),
        ))
        .into_any_element()
}

/// A GFM table.
///
/// Columns are content-proportional with a per-column floor: each cell is
/// shaped unwrapped to get its max-content width, and the flex resolution does
/// the rest. When even the floors no longer fit, the table scrolls sideways
/// rather than crushing every column into per-character wrapping.
#[expect(
    clippy::too_many_arguments,
    reason = "a table, its overlay, and what paints them"
)]
fn table(
    align: &[Align],
    header: &[Text],
    rows: &[Vec<Text>],
    overlay: Overlay,
    typography: &Typography,
    theme: &Theme,
    window: &mut Window,
    _cx: &App,
) -> AnyElement {
    let ix = overlay.block;
    let all: Vec<&[Text]> = std::iter::once(header)
        .filter(|row| !row.is_empty())
        .chain(rows.iter().map(|row| row.as_slice()))
        .collect();
    let columns = all.iter().map(|row| row.len()).max().unwrap_or(0);
    if columns == 0 {
        return gpui::Empty.into_any_element();
    }
    let has_header = !header.is_empty();

    let text_system = window.text_system();
    let mut flats: Vec<Vec<Option<Flat>>> = Vec::with_capacity(all.len());
    let mut content = vec![0.0f32; columns];
    for (r, row) in all.iter().enumerate() {
        let weight = if has_header && r == 0 {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };
        let mut out = Vec::with_capacity(columns);
        for (c, natural) in content.iter_mut().enumerate() {
            let Some(cell) = row.get(c) else {
                out.push(None);
                continue;
            };
            let flat = flatten(cell, weight, theme);
            if !flat.text.is_empty() {
                let width = f32::from(
                    text_system
                        .shape_line(
                            flat.text.clone(),
                            px(typography.body.size()),
                            &flat.runs,
                            None,
                        )
                        .width(),
                );
                *natural = natural.max(width);
            }
            out.push(Some(flat));
        }
        flats.push(out);
    }

    let naturals: Vec<f32> = content
        .iter()
        .map(|width| width.max(TABLE_MIN_COLUMN_CONTENT) + 2.0 * TABLE_CELL_PADDING)
        .collect();
    let minimums: Vec<f32> = naturals
        .iter()
        .map(|natural| natural.min(TABLE_MIN_COLUMN_WIDTH))
        .collect();
    let hairline = theme.hairline(0.10);

    let mut inner = div()
        .flex()
        .flex_col()
        .w_full()
        .min_w(px(minimums.iter().sum::<f32>()));
    for (r, row) in flats.into_iter().enumerate() {
        if r > 0 {
            inner = inner.child(div().flex_none().h(px(TABLE_DIVIDER)).w_full().bg(hairline));
        }
        let mut row_el = div().flex().flex_row();
        for (c, cell) in row.into_iter().enumerate() {
            let mut cell_el = div()
                .flex_grow(naturals[c])
                .flex_shrink(naturals[c])
                .flex_basis(px(0.0))
                .min_w(px(minimums[c]))
                .p(px(TABLE_CELL_PADDING))
                .text_size(px(typography.body.size()))
                .line_height(px(typography.body.line_height()));
            cell_el = match align.get(c).copied().unwrap_or_default() {
                Align::Left => cell_el,
                Align::Center => cell_el.text_center(),
                Align::Right => cell_el.text_right(),
            };
            if let Some(flat) = cell {
                // `all` drops an empty header, so a table without one starts at
                // part row 1 — row 0 is the header slot whether or not it is
                // filled.
                let row = if has_header { r } else { r + 1 };
                let len = flat.source_len;
                cell_el = cell_el.child(painted_text(
                    flat,
                    len,
                    typography.body.size(),
                    typography.body.line_height(),
                    overlay.at(Part::Cell { row, column: c }),
                    theme,
                ));
            }
            row_el = row_el.child(cell_el);
        }
        inner = inner.child(row_el);
    }

    super::scroll::horizontal(
        format!("md-table-scroll-{ix}"),
        div()
            .id(ElementId::named_usize("md-table", ix))
            .w_full()
            .child(inner),
    )
    .into_any_element()
}
