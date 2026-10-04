//! Source-preserving document editing adapted from Bezel 4a7505ab (MIT).
//! See third_party_licenses/bezel.md.
//!
//! Kit's Editor edits source and its TextView is read-only; neither provides a
//! caret in rendered Markdown, marked text editing or editable table cells.
//! Bezel's document and painting supply that missing interaction. Kit retains
//! the caller's sole buffer, undo history and existing draft/save subscriptions.
mod buffer;
mod commands;
mod composer;
mod input;
mod keys;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod menu;
mod pointer;
mod serialize;
mod source;
#[cfg(test)]
mod tests;

use crate::content::markdown::{
    self, Block, BlockKind, BlockLayouts, Cursor, Doc, Editing, Mark, Selection, doc::Text,
    parse::parse,
};
use buffer::Buffer;
#[cfg(test)]
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::{
    RopeExt as _,
    input::{
        Copy, Cut, Editor as SourceEditor, EditorState, Paste, Position, Redo, SelectAll, Undo,
    },
    scroll::ScrollableElement,
};
use gpui_kit::{
    AnyElement, App, Context, DispatchPhase, ElementInputHandler, Entity, FocusHandle, Focusable,
    IntoElement, MouseButton, MouseMoveEvent, Pixels, Point, Render, RenderOnce, ScrollHandle,
    SharedString, Subscription, Task, Window, canvas, div, point, prelude::*, px,
};
use std::{ops::Range, rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Document,
    Source,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    Find,
}

struct Composition {
    doc: Doc,
    selection: Selection,
}

pub struct State {
    input: Buffer,
    source: SharedString,
    doc: Doc,
    map: source::Map,
    selection: Selection,
    source_selection: Range<usize>,
    mode: Mode,
    marked: Option<Range<usize>>,
    composition: Option<Composition>,
    stored: Vec<Mark>,
    transaction: bool,
    layouts: BlockLayouts,
    scroll: ScrollHandle,
    focus: FocusHandle,
    dragging: bool,
    pointer: Point<Pixels>,
    pressed: Option<(gpui_kit::base::input::TextareaLink, Point<Pixels>)>,
    readonly: bool,
    label: Option<SharedString>,
    reveal: bool,
    caret_on: bool,
    blink: Option<Task<()>>,
    _source: Subscription,
    _blur: Option<Subscription>,
}

impl State {
    #[cfg(test)]
    pub fn new(
        source: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = source.into();
        let input = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .default_value(source)
        });
        Self::from_input(&input, cx)
    }

    /// Attach to the existing resource editor; drafts and saves use this buffer.
    pub fn from_input(input: &Entity<EditorState>, cx: &mut Context<Self>) -> Self {
        Self::from_buffer(Buffer::Editor(input.clone()), cx)
    }

    /// Render the conversation's existing draft without another buffer or history.
    #[cfg(test)]
    pub fn from_textarea(input: &Entity<TextareaState>, cx: &mut Context<Self>) -> Self {
        Self::from_buffer(Buffer::Textarea(input.clone()), cx)
    }

    /// Project the rendered caret onto the caller's existing source buffer.
    pub fn source_cursor_position(&self, cx: &App) -> Option<Position> {
        if self.mode != Mode::Document {
            return None;
        }
        let offset = self.map.offset(self.cursor())?;
        Some(match &self.input {
            Buffer::Editor(input) => input.read(cx).text().offset_to_position(offset),
            #[cfg(test)]
            Buffer::Textarea(input) => input.read(cx).text().offset_to_position(offset),
        })
    }

    fn from_buffer(input: Buffer, cx: &mut Context<Self>) -> Self {
        keys::init(cx);
        let source = input.value(cx);
        let doc = if input.textarea().is_some() {
            composer::draft(&source)
        } else {
            document(&source)
        };
        let map = source::Map::new(&source, &doc);
        let changes = match &input {
            Buffer::Editor(input) => cx.observe(input, |state, _, cx| {
                state.sync(cx);
                cx.notify();
            }),
            #[cfg(test)]
            Buffer::Textarea(input) => cx.observe(input, |state, _, cx| {
                state.sync(cx);
                cx.notify();
            }),
        };
        let source_selection = input.selection(cx);
        let selection = if input.textarea().is_some() {
            Selection::new(
                map.cursor(source_selection.start, &doc),
                map.cursor(source_selection.end, &doc),
            )
        } else {
            Selection::default()
        };
        let focus = if input.textarea().is_some() {
            input.focus_handle(cx)
        } else {
            cx.focus_handle()
        };
        cx.on_release(|state, cx| state.commit(cx)).detach();
        Self {
            input,
            source,
            doc,
            map,
            selection,
            source_selection,
            mode: Mode::Document,
            marked: None,
            composition: None,
            stored: Vec::new(),
            transaction: false,
            layouts: BlockLayouts::default(),
            scroll: ScrollHandle::new(),
            focus,
            dragging: false,
            pointer: point(px(0.), px(0.)),
            pressed: None,
            readonly: false,
            label: None,
            reveal: false,
            caret_on: true,
            blink: None,
            _source: changes,
            _blur: None,
        }
    }

    pub fn source(&self, cx: &App) -> SharedString {
        self.input.value(cx)
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn can_copy(&self, cx: &App) -> bool {
        match self.mode {
            Mode::Source => !self.input.selection(cx).is_empty(),
            Mode::Document if self.input.textarea().is_some() => !self.selected_source().is_empty(),
            Mode::Document => !self.selection.is_collapsed(),
        }
    }
    pub fn can_cut_paste(&self) -> bool {
        !self.readonly
    }

    pub fn command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        let restore_focus = matches!(command, Command::Copy | Command::Cut | Command::Paste);
        match command {
            Command::Undo => self.undo(&Undo, window, cx),
            Command::Redo => self.redo(&Redo, window, cx),
            Command::Find => {
                self.set_mode(Mode::Source, window, cx);
                self.input.find(cx);
            }
            command if self.mode == Mode::Source => {
                if matches!(command, Command::Copy) || !self.readonly {
                    self.input.command(command, window, cx);
                }
            }
            Command::Copy => self.copy(window, cx),
            Command::Cut => {
                if !self.readonly && self.can_copy(cx) {
                    self.copy(window, cx);
                    if self.input.textarea().is_some() {
                        self.replace_source("", window, cx);
                    } else {
                        self.insert("", window, cx);
                    }
                }
            }
            Command::Paste => self.paste(window, cx),
        }
        if restore_focus {
            self.focus_handle(cx).focus(window, cx);
        }
    }

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.cancel_composition(cx);
        self.commit(cx);
        self.dragging = false;
        self.blink = None;
        self.sync(cx);
        if mode == Mode::Source {
            let (start, end) = self.selection.ordered();
            if let Some(range) = self
                .map
                .offset(start)
                .zip(self.map.offset(end))
                .map(|(start, end)| start..end)
            {
                self.input.select(range, cx);
            }
        } else {
            self.selection =
                Selection::at(self.map.cursor(self.input.selection(cx).end, &self.doc));
        }
        self.mode = mode;
        self.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn cursor(&self) -> Cursor {
        self.selection.head.clamp(&self.doc)
    }
    fn caret_text(&self) -> Option<&Text> {
        let at = self.cursor();
        self.doc.blocks.get(at.block)?.text_at(at.part)
    }
    fn place(&mut self, at: Cursor) {
        self.selection = Selection::at(at.clamp(&self.doc));
    }
    fn notify_caret(&mut self, cx: &mut Context<Self>) {
        self.sync_selection(cx);
        self.reveal = true;
        self.caret_on = true;
        self.blink = None;
        cx.notify();
    }

    fn cancel_composition(&mut self, cx: &mut Context<Self>) {
        if let Some(original) = self.composition.take() {
            self.doc = original.doc;
            self.selection = original.selection;
        }
        if self.marked.take().is_some() {
            cx.notify();
        }
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let source = self.source(cx);
        if source == self.source {
            let selected = self.input.selection(cx);
            if self.input.textarea().is_some()
                && self.composition.is_none()
                && selected != self.source_selection
            {
                self.selection = Selection::new(
                    self.map.cursor(selected.start, &self.doc),
                    self.map.cursor(selected.end, &self.doc),
                );
                self.source_selection = selected;
            }
            return;
        }
        self.cancel_composition(cx);
        self.commit(cx);
        self.doc = if self.input.textarea().is_some() {
            composer::draft(&source)
        } else {
            document(&source)
        };
        self.map = source::Map::new(&source, &self.doc);
        self.source = source;
        self.source_selection = self.input.selection(cx);
        self.selection = if self.input.textarea().is_some() {
            Selection::new(
                self.map.cursor(self.source_selection.start, &self.doc),
                self.map.cursor(self.source_selection.end, &self.doc),
            )
        } else {
            Selection::at(self.map.cursor(self.source_selection.end, &self.doc))
        };
        self.stored.clear();
        cx.notify();
    }

    fn apply(&mut self, before: Doc, atomic: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly {
            self.doc = before;
            return;
        }
        let Some(patch) = self.map.patch(&self.source, &before, &self.doc) else {
            self.notify_caret(cx);
            return;
        };
        if atomic {
            self.commit(cx);
        }
        if !self.transaction {
            self.input.begin(cx);
            self.transaction = true;
        }
        let mut updated = self.source.to_string();
        updated.replace_range(patch.range.clone(), &patch.text);
        // Set the expected source before notifying Kit observers, so a view's
        // own edit retains temporary trailing spaces and an empty new paragraph.
        self.source = updated.into();
        self.map = source::Map::new(&self.source, &self.doc);
        self.input.replace(patch.range, patch.text, window, cx);
        if atomic {
            self.commit(cx);
        }
        self.notify_caret(cx);
    }

    fn commit(&mut self, cx: &mut App) {
        if std::mem::take(&mut self.transaction) {
            self.input.commit(cx);
        }
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        self.commit(cx);
        self.input.undo(window, cx);
        self.sync(cx);
        self.focus_handle(cx).focus(window, cx);
    }
    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if self.readonly {
            return;
        }
        self.cancel_composition(cx);
        self.commit(cx);
        self.input.redo(window, cx);
        self.sync(cx);
        self.focus_handle(cx).focus(window, cx);
    }

    fn document(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.focus.is_focused(window) && !self.readonly;
        if focused && self.selection.is_collapsed() {
            if self.blink.is_none() {
                self.blink = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    let _ = this.update(cx, |state, cx| {
                        state.blink = None;
                        state.caret_on = !state.caret_on;
                        cx.notify();
                    });
                }));
            }
        } else {
            self.blink = None;
            self.caret_on = true;
        }
        if self._blur.is_none() {
            self._blur = Some(cx.on_blur(&self.focus, window, |state, _, cx| {
                state.cancel_composition(cx);
                state.commit(cx);
                state.dragging = false;
                state.pressed = None;
                state.blink = None;
                cx.notify();
            }));
        }
        if self.dragging {
            let entity = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |state, cx| state.autoscroll(cx));
            });
        }
        if std::mem::take(&mut self.reveal) {
            let entity = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |state, cx| {
                    let Some((position, height)) = state.layouts.position(state.cursor()) else {
                        return;
                    };
                    let bounds = state.scroll.bounds();
                    let delta = if position.y < bounds.top() {
                        bounds.top() - position.y
                    } else if position.y + height > bounds.bottom() {
                        bounds.bottom() - position.y - height
                    } else {
                        px(0.)
                    };
                    if delta != px(0.) {
                        let offset = state.scroll.offset();
                        state.scroll.set_offset(point(
                            offset.x,
                            (offset.y + delta).clamp(-state.scroll.max_offset().y, px(0.)),
                        ));
                        cx.notify();
                    }
                });
            });
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        let writable = !self.readonly;
        let input = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                if writable {
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, entity.clone()),
                        cx,
                    );
                }
            },
        )
        .absolute()
        .size_full();
        let dragging = self.dragging.then(|| {
            let entity = cx.weak_entity();
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble
                            && event.pressed_button == Some(MouseButton::Left)
                        {
                            let _ = entity.update(cx, |state, cx| state.moved(event.position, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });
        let annotations = self.annotations(cx);
        let placeholder = self
            .input
            .textarea()
            .map(|input| input.read(cx).presentation().placeholder().clone());
        let content = markdown::render_with(
            &self.doc,
            Editing {
                selection: Some(self.selection),
                caret_on: focused && self.caret_on,
                layouts: Some(&self.layouts),
                annotations: &annotations,
                placeholder: placeholder.clone(),
                // Document clicks place the caret, including linked text.
                on_link: Some(Rc::new(|_, _, _| {})),
                ..Editing::default()
            },
            window,
            cx,
        );
        let surface = div()
            .id("markdown-document")
            .debug_selector(|| "markdown-document".into())
            .w_full()
            .when(self.input.textarea().is_none(), |surface| {
                surface.size_full().min_h_0().p(px(16.))
            })
            .when(self.input.textarea().is_some(), |surface| {
                let line = px(markdown::typography::Typography::of(cx).body.line_height());
                let entity = cx.entity();
                surface
                    .min_h(line * 2.)
                    .max_h(line * 6.)
                    .role(gpui_kit::Role::MultilineTextInput)
                    .aria_value(self.source.clone())
                    .on_a11y_action(
                        gpui_kit::AccessibleAction::SetValue,
                        move |data, window, cx| {
                            entity.update(cx, |state, cx| {
                                state.accessibility_value(data, window, cx)
                            });
                        },
                    )
                    .when_some(placeholder.clone(), |surface, placeholder| {
                        surface.aria_placeholder(placeholder)
                    })
                    .when_some(self.label.clone(), |surface, label| {
                        surface.aria_label(label)
                    })
            })
            .track_scroll(&self.scroll)
            .overflow_y_scrollbar()
            .relative()
            .cursor_text()
            .key_context(if self.input.textarea().is_some() {
                if self.selection.is_collapsed() {
                    keys::COMPOSER
                } else {
                    keys::SELECTION
                }
            } else {
                keys::CONTEXT
            })
            .track_focus(&self.focus.clone().tab_stop(true))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|state, event, _, cx| {
                    state.release_link(event, cx);
                    state.release(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|state, _, _, cx| {
                    state.pressed = None;
                    state.release(cx);
                }),
            )
            .on_action(
                cx.listener(|state, _: &Copy, window, cx| state.command(Command::Copy, window, cx)),
            )
            .on_action(
                cx.listener(|state, _: &Cut, window, cx| state.command(Command::Cut, window, cx)),
            )
            .on_action(
                cx.listener(|state, _: &Paste, window, cx| {
                    state.command(Command::Paste, window, cx)
                }),
            )
            .on_action(cx.listener(|state, _: &SelectAll, _, cx| {
                state.select_all(cx);
            }))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::soft_break))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::composer_backspace))
            .on_action(cx.listener(Self::composer_up))
            .on_action(cx.listener(Self::composer_down))
            .on_action(cx.listener(Self::composer_indent))
            .on_action(cx.listener(Self::composer_outdent))
            .on_action(cx.listener(|state, _: &keys::Bold, window, cx| {
                state.toggle_mark(Mark::Bold, window, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::Italic, window, cx| {
                state.toggle_mark(Mark::Italic, window, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::Code, window, cx| {
                state.toggle_mark(Mark::Code, window, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::Strike, window, cx| {
                state.toggle_mark(Mark::Strike, window, cx)
            }))
            .on_action(
                cx.listener(|state, _: &keys::Left, _, cx| state.motion(Cursor::left, false, cx)),
            )
            .on_action(
                cx.listener(|state, _: &keys::Right, _, cx| state.motion(Cursor::right, false, cx)),
            )
            .on_action(cx.listener(|state, _: &keys::SelectLeft, _, cx| {
                state.motion(Cursor::left, true, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::SelectRight, _, cx| {
                state.motion(Cursor::right, true, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::WordLeft, _, cx| {
                state.motion(Cursor::word_left, false, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::WordRight, _, cx| {
                state.motion(Cursor::word_right, false, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::SelectWordLeft, _, cx| {
                state.motion(Cursor::word_left, true, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::SelectWordRight, _, cx| {
                state.motion(Cursor::word_right, true, cx)
            }))
            .on_action(cx.listener(|state, _: &keys::Home, _, cx| {
                state.motion(|at, _| at.home(), false, cx)
            }))
            .on_action(
                cx.listener(|state, _: &keys::End, _, cx| state.motion(Cursor::end, false, cx)),
            )
            .on_action(cx.listener(|state, _: &keys::SelectHome, _, cx| {
                state.motion(|at, _| at.home(), true, cx)
            }))
            .on_action(
                cx.listener(|state, _: &keys::SelectEnd, _, cx| {
                    state.motion(Cursor::end, true, cx)
                }),
            )
            .on_action(cx.listener(|state, _: &keys::Up, _, cx| state.vertical(false, false, cx)))
            .on_action(cx.listener(|state, _: &keys::Down, _, cx| state.vertical(true, false, cx)))
            .on_action(
                cx.listener(|state, _: &keys::SelectUp, _, cx| state.vertical(false, true, cx)),
            )
            .on_action(
                cx.listener(|state, _: &keys::SelectDown, _, cx| state.vertical(true, true, cx)),
            )
            .child(input)
            .children(dragging)
            .child(content);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let surface = surface.on_mouse_down(MouseButton::Right, cx.listener(Self::context_menu));
        surface.into_any_element()
    }
}

fn document(source: &str) -> Doc {
    let mut doc = parse(source);
    if doc.blocks.is_empty() {
        doc.blocks
            .push(Block::new(BlockKind::Paragraph(Text::default())));
    }
    doc
}

impl Focusable for State {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.mode == Mode::Source {
            self.input.focus_handle(cx)
        } else {
            self.focus.clone()
        }
    }
}

impl Render for State {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.input.textarea().is_some() {
            let content = if self.mode == Mode::Document && self.document_active(window, cx) {
                self.sync_selection(cx);
                self.document(window, cx)
            } else {
                self.input.render(self.readonly, self.label.clone())
            };
            return div()
                .w_full()
                .capture_action(cx.listener(Self::undo))
                .capture_action(cx.listener(Self::redo))
                .child(content)
                .into_any_element();
        }
        let content = match self.mode {
            Mode::Document => self.document(window, cx),
            Mode::Source => self.input.render(self.readonly, self.label.clone()),
        };
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .capture_action(cx.listener(Self::undo))
            .capture_action(cx.listener(Self::redo))
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }
}

#[derive(IntoElement)]
pub struct View {
    state: Entity<State>,
    readonly: bool,
    label: Option<SharedString>,
}
impl View {
    pub fn new(state: &Entity<State>) -> Self {
        Self {
            state: state.clone(),
            readonly: false,
            label: None,
        }
    }
    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}
impl RenderOnce for View {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, cx| {
            if self.readonly && !state.readonly {
                state.cancel_composition(cx);
                state.commit(cx);
            }
            state.readonly = self.readonly;
            state.label = self.label;
        });
        self.state
    }
}
