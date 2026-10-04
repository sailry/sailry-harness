use super::{
    Kind, Line,
    selection::{self, Cursor, Selection},
    text::{Layouts, Text},
};
use gpui_kit::{
    component::{
        ActiveTheme, h_flex,
        highlighter::HighlightTheme,
        scroll::{ScrollableElement, ScrollableMask},
    },
    prelude::FluentBuilder as _,
    *,
};
use std::{ops::Range, rc::Rc, sync::Arc};

gpui_kit::actions!(sailry_diff, [Copy, SelectAll]);
struct Bindings;
impl Global for Bindings {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    kind: Kind,
    old: Option<usize>,
    new: Option<usize>,
    range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Data {
    source: SharedString,
    rows: Vec<Row>,
    longest: Option<usize>,
}

impl Data {
    fn new(lines: &[Line<'_>]) -> Self {
        let mut source = String::new();
        let rows = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                if index > 0 {
                    source.push('\n');
                }
                let start = source.len();
                source.push_str(line.text);
                Row {
                    kind: line.kind,
                    old: line.old,
                    new: line.new,
                    range: start..source.len(),
                }
            })
            .collect();
        Self {
            source: source.into(),
            longest: lines
                .iter()
                .enumerate()
                .max_by_key(|(_, line)| line.text.len())
                .map(|(index, _)| index),
            rows,
        }
    }
    fn text(&self, row: usize) -> &str {
        &self.source[self.rows[row].range.clone()]
    }
    fn clamp(&self, cursor: Cursor) -> Cursor {
        if self.rows.is_empty() {
            return Cursor { row: 0, byte: 0 };
        }
        let row = cursor.row.min(self.rows.len() - 1);
        Cursor {
            row,
            byte: selection::clamp(self.text(row), cursor.byte),
        }
    }
    fn copied(&self, selection: Selection) -> String {
        if self.rows.is_empty() {
            return String::new();
        }
        let start = self.clamp(selection.anchor.min(selection.cursor));
        let end = self.clamp(selection.anchor.max(selection.cursor));
        self.source[self.rows[start.row].range.start + start.byte
            ..self.rows[end.row].range.start + end.byte]
            .to_owned()
    }
}

pub(crate) struct State {
    data: Data,
    language: String,
    selection: Option<Selection>,
    dragging: bool,
    pointer: Point<Pixels>,
    layouts: Layouts,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    syntax: Option<Syntax>,
}

struct Syntax {
    theme: Arc<HighlightTheme>,
    spans: Rc<Vec<(Range<usize>, HighlightStyle)>>,
}

impl State {
    pub(crate) fn new(lines: &[Line<'_>], language: &str, cx: &mut Context<Self>) -> Self {
        if !cx.has_global::<Bindings>() {
            let (copy, all) = if cfg!(target_os = "macos") {
                ("cmd-c", "cmd-a")
            } else {
                ("ctrl-c", "ctrl-a")
            };
            cx.bind_keys([
                KeyBinding::new(copy, Copy, Some("SailryDiff")),
                KeyBinding::new(all, SelectAll, Some("SailryDiff")),
            ]);
            cx.set_global(Bindings);
        }
        Self {
            data: Data::new(lines),
            language: language.into(),
            selection: None,
            dragging: false,
            pointer: Point::default(),
            layouts: Layouts::default(),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            syntax: None,
        }
    }
    pub(crate) fn set_lines(&mut self, lines: &[Line<'_>], cx: &mut Context<Self>) {
        let data = Data::new(lines);
        if self.data == data {
            return;
        }
        self.data = data;
        self.syntax = None;
        self.selection = self.selection.map(|selection| Selection {
            anchor: self.data.clamp(selection.anchor),
            cursor: self.data.clamp(selection.cursor),
        });
        cx.notify();
    }
    pub(crate) fn len(&self) -> usize {
        self.data.rows.len()
    }
    fn moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        self.pointer = position;
        if let Some(cursor) = self.layouts.hit(position) {
            let cursor = self.data.clamp(cursor);
            let selection = self.selection.map(|selection| Selection {
                cursor,
                ..selection
            });
            if selection != self.selection {
                self.selection = selection;
                cx.notify();
            }
        }
    }
    fn autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let scroll = self.scroll.0.borrow().base_handle.clone();
        let bounds = scroll.bounds();
        let delta = if self.pointer.y < bounds.top() {
            self.pointer.y - bounds.top()
        } else if self.pointer.y > bounds.bottom() {
            self.pointer.y - bounds.bottom()
        } else {
            px(0.)
        };
        let before = scroll.offset();
        let next = point(
            before.x,
            (before.y - delta.clamp(px(-30.), px(30.))).clamp(-scroll.max_offset().y, px(0.)),
        );
        if before != next {
            scroll.set_offset(next);
            cx.notify();
        }
        // The latest layouts arrive after the previous scroll's paint. Resolve
        // the endpoint even when that scroll has just reached the boundary.
        self.moved(self.pointer, cx);
    }
    fn row(&self, index: usize, highlights: &[(Range<usize>, HighlightStyle)], cx: &App) -> Div {
        let row = &self.data.rows[index];
        let text = self.data.text(index);
        let theme = cx.theme();
        let (sign, tone) = match row.kind {
            Kind::Added => ("+", Some(theme.success)),
            Kind::Removed => ("-", Some(theme.danger)),
            _ => (" ", None),
        };
        let muted = matches!(row.kind, Kind::Context | Kind::Hunk | Kind::Meta);
        let spans = highlights
            [highlights.partition_point(|(range, _)| range.end <= row.range.start)..]
            .iter()
            .take_while(|(range, _)| range.start < row.range.end)
            .filter_map(|(range, style)| {
                let start = range.start.max(row.range.start);
                let end = range.end.min(row.range.end);
                (start < end).then_some((start - row.range.start..end - row.range.start, *style))
            })
            .collect::<Vec<_>>();
        let spans = selected_spans(
            spans,
            self.selection
                .and_then(|selection| selection.range(index, text.len())),
            text.len(),
            theme.selection,
        );
        let number = |value: Option<usize>| {
            div()
                .w(px(30.))
                .flex_shrink_0()
                .text_right()
                .text_color(theme.muted_foreground)
                .child(value.map(|value| value.to_string()).unwrap_or_default())
        };
        let body = h_flex()
            .debug_selector(move || format!("diff-line-{index}"))
            .items_start()
            .gap(px(8.))
            .pr(px(10.))
            .py(px(1.))
            .h(theme.mono_font_size * 1.5 + px(2.))
            .min_w_full()
            .when_some(tone, |row, tone| row.bg(tone.opacity(0.10)))
            .when(matches!(row.kind, Kind::Hunk | Kind::Meta), |row| {
                row.bg(theme.foreground.opacity(0.02))
            });
        let body = if matches!(row.kind, Kind::Hunk | Kind::Meta) {
            body.child(div().w(px(88.)).flex_shrink_0())
        } else {
            body.child(number(row.old)).child(number(row.new)).child(
                div()
                    .w(px(12.))
                    .flex_shrink_0()
                    .text_color(tone.unwrap_or(theme.muted_foreground))
                    .child(sign),
            )
        };
        body.child(
            div()
                .flex_shrink_0()
                .text_color(if muted {
                    theme.muted_foreground
                } else {
                    theme.foreground
                })
                .child(Text {
                    text: StyledText::new(text.to_owned()).with_highlights(spans),
                    row: index,
                    layouts: self.layouts.clone(),
                }),
        )
    }
}

impl Focusable for State {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for State {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.layouts.0.borrow_mut().clear();
        if self.dragging {
            let view = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = view.update(cx, |view, cx| view.autoscroll(cx));
            });
            window.request_animation_frame();
        }
        let view = cx.entity();
        let theme = cx.theme().highlight_theme.clone();
        if self
            .syntax
            .as_ref()
            .is_none_or(|syntax| !Arc::ptr_eq(&syntax.theme, &theme))
        {
            self.syntax = Some(Syntax {
                theme,
                spans: Rc::new(crate::content::syntax::highlight(
                    &self.language,
                    &self.data.source,
                    cx,
                )),
            });
        }
        let highlights = self.syntax.as_ref().unwrap().spans.clone();
        let list = uniform_list("diff-lines", self.len(), move |range, _, cx| {
            view.update(cx, |view, cx| {
                range
                    .map(|index| view.row(index, &highlights, cx))
                    .collect::<Vec<_>>()
            })
        })
        .track_scroll(&self.scroll)
        .with_width_from_item(self.data.longest)
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .size_full();
        let listener = self.dragging.then(|| {
            let view = cx.weak_entity();
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble
                            && event.pressed_button == Some(MouseButton::Left)
                        {
                            let _ = view.update(cx, |view, cx| view.moved(event.position, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });
        div()
            .id("diff-surface")
            .debug_selector(|| "diff-viewport".into())
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(cx.theme().mono_font_size)
            .line_height(cx.theme().mono_font_size * 1.5)
            .whitespace_nowrap()
            .key_context("SailryDiff")
            .track_focus(&self.focus)
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    let Some(cursor) = view.layouts.hit(event.position) else {
                        return;
                    };
                    let cursor = view.data.clamp(cursor);
                    view.focus.focus(window, cx);
                    view.pointer = event.position;
                    view.dragging = true;
                    view.selection = Some(if event.modifiers.shift {
                        Selection {
                            cursor,
                            ..view.selection.unwrap_or(Selection {
                                anchor: cursor,
                                cursor,
                            })
                        }
                    } else if event.click_count >= 3 {
                        Selection {
                            anchor: Cursor { byte: 0, ..cursor },
                            cursor: Cursor {
                                byte: view.data.text(cursor.row).len(),
                                ..cursor
                            },
                        }
                    } else if event.click_count == 2 {
                        let range = selection::word(view.data.text(cursor.row), cursor.byte);
                        Selection {
                            anchor: Cursor {
                                byte: range.start,
                                ..cursor
                            },
                            cursor: Cursor {
                                byte: range.end,
                                ..cursor
                            },
                        }
                    } else {
                        Selection {
                            anchor: cursor,
                            cursor,
                        }
                    });
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.dragging = false;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.dragging = false;
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|view, _: &Copy, _, cx| {
                if let Some(selection) = view.selection {
                    let text = view.data.copied(selection);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
            }))
            .on_action(cx.listener(|view, _: &SelectAll, _, cx| {
                if let Some(row) = view.data.rows.len().checked_sub(1) {
                    view.selection = Some(Selection {
                        anchor: Cursor { row: 0, byte: 0 },
                        cursor: Cursor {
                            row,
                            byte: view.data.text(row).len(),
                        },
                    });
                    cx.notify();
                }
            }))
            .child(list)
            .scrollbar(
                &self.scroll,
                gpui_kit::component::scroll::ScrollbarAxis::Both,
            )
            .child(ScrollableMask::new(Axis::Vertical, &self.scroll).id("diff-vertical"))
            .child(ScrollableMask::new(Axis::Horizontal, &self.scroll).id("diff-horizontal"))
            .children(listener)
    }
}

/// StyledText requires ordered, disjoint runs; selection overlays syntax ink.
fn selected_spans(
    spans: Vec<(Range<usize>, HighlightStyle)>,
    selected: Option<Range<usize>>,
    length: usize,
    color: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(selected) = selected else {
        return spans;
    };
    let mut boundaries = vec![0, length, selected.start, selected.end];
    boundaries.extend(spans.iter().flat_map(|(range, _)| [range.start, range.end]));
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut syntax = spans.iter().peekable();
    boundaries
        .windows(2)
        .filter_map(|pair| {
            let range = pair[0]..pair[1];
            if range.is_empty() {
                return None;
            }
            while syntax
                .peek()
                .is_some_and(|(span, _)| span.end <= range.start)
            {
                syntax.next();
            }
            let mut style = syntax
                .peek()
                .filter(|(span, _)| span.start <= range.start && span.end >= range.end)
                .map(|(_, style)| *style)
                .unwrap_or_default();
            if range.start < selected.end && range.end > selected.start {
                style.background_color = Some(color);
            }
            Some((range, style))
        })
        .collect()
}

#[derive(IntoElement)]
struct Inline {
    id: SharedString,
    data: Data,
    language: String,
}
impl RenderOnce for Inline {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let lines = self
            .data
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| Line {
                kind: row.kind,
                old: row.old,
                new: row.new,
                text: self.data.text(index),
            })
            .collect::<Vec<_>>();
        let view = window.use_keyed_state(self.id.clone(), cx, |_, cx| {
            State::new(&lines, &self.language, cx)
        });
        view.update(cx, |view, cx| view.set_lines(&lines, cx));
        div()
            .debug_selector(move || format!("diff-rows-{}", self.id))
            .w_full()
            .h(
                (cx.theme().mono_font_size * 1.5 + px(2.))
                    * self.data.rows.len().clamp(1, 5) as f32,
            )
            .child(view)
    }
}

pub(crate) fn rows(id: &str, lines: &[Line<'_>], language: &str, _: &App) -> AnyElement {
    Inline {
        id: format!("diff-{id}").into(),
        data: Data::new(lines),
        language: language.into(),
    }
    .into_any_element()
}

pub(crate) fn surface(id: &str, state: &Entity<State>) -> AnyElement {
    let selector = format!("diff-rows-diff-{id}");
    div()
        .debug_selector(move || selector.clone())
        .size_full()
        .child(state.clone())
        .into_any_element()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
