//! A retained, selectable surface over the Bezel presentation model.
//!
//! Source bytes remain authoritative. Parsing runs only when source changes;
//! pointer moves, theme changes and streaming frames reuse the same document.
use std::rc::Rc;

use gpui_kit::{
    App, ClipboardItem, Context, DispatchPhase, ElementId, Entity, FocusHandle, Focusable, Global,
    IntoElement, KeyBinding, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    Render, RenderOnce, ScrollHandle, SharedString, Window, canvas, div, point, prelude::*, px,
};
use unicode_segmentation::UnicodeSegmentation;

use super::render::{OnFile, OnImage, OnLink};
use super::{BlockLayouts, Cursor, Doc, Editing, Selection, parse_ranges, render_with};

gpui_kit::actions!(sailry_markdown, [Copy, SelectAll]);
struct Bindings;
impl Global for Bindings {}

pub struct State {
    source: SharedString,
    document: Rc<Doc>,
    layouts: BlockLayouts,
    selection: Option<Selection>,
    dragging: bool,
    pointer: Point<Pixels>,
    focus: FocusHandle,
    on_link: Option<OnLink>,
    on_image: Option<OnImage>,
    on_file: Option<OnFile>,
    scroll: Option<Viewport>,
}

#[derive(Clone)]
enum Viewport {
    Scroll(ScrollHandle),
    List(ListState),
}

impl Viewport {
    fn bounds(&self) -> gpui_kit::Bounds<Pixels> {
        match self {
            Self::Scroll(handle) => handle.bounds(),
            Self::List(handle) => handle.viewport_bounds(),
        }
    }
    fn scroll_by(&self, delta: f32) -> bool {
        match self {
            Self::Scroll(handle) => {
                let before = handle.offset();
                let next = point(
                    before.x,
                    (before.y - px(delta)).clamp(-handle.max_offset().y, px(0.)),
                );
                if next == before {
                    return false;
                }
                handle.set_offset(next);
            }
            Self::List(handle) => {
                let before = handle.scroll_px_offset_for_scrollbar();
                let next = point(
                    before.x,
                    (before.y - px(delta)).clamp(-handle.max_offset_for_scrollbar().y, px(0.)),
                );
                if next == before {
                    return false;
                }
                handle.pause_following_tail();
                handle.set_offset_from_scrollbar(next);
            }
        }
        true
    }
}

impl State {
    pub fn new(source: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        if !cx.has_global::<Bindings>() {
            let (copy, select) = if cfg!(target_os = "macos") {
                ("cmd-c", "cmd-a")
            } else {
                ("ctrl-c", "ctrl-a")
            };
            cx.bind_keys([
                KeyBinding::new(copy, Copy, Some("SailryMarkdown")),
                KeyBinding::new(select, SelectAll, Some("SailryMarkdown")),
            ]);
            cx.set_global(Bindings);
        }
        let source = source.into();
        let parsed = parse_ranges(source.as_ref());
        Self {
            document: Rc::new(parsed.doc),
            source,
            layouts: BlockLayouts::default(),
            selection: None,
            dragging: false,
            pointer: point(px(0.), px(0.)),
            focus: cx.focus_handle(),
            on_link: None,
            on_image: None,
            on_file: None,
            scroll: None,
        }
    }

    #[cfg(test)]
    pub fn source(&self) -> &str {
        self.source.as_ref()
    }
    pub fn selected_text(&self) -> String {
        self.selection
            .map(|selection| super::selectable::copied(&self.document, selection))
            .unwrap_or_default()
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection = Some(Selection::all(&self.document));
        cx.notify();
    }

    pub fn set_source(&mut self, source: impl Into<SharedString>, cx: &mut Context<Self>) {
        let source = source.into();
        if source == self.source {
            return;
        }
        let parsed = parse_ranges(source.as_ref());
        self.document = Rc::new(if self.on_image.is_some() {
            super::images::expand(parsed.doc)
        } else {
            parsed.doc
        });
        self.source = source;
        self.selection = self
            .selection
            .map(|selection| selection.clamp(&self.document));
        cx.notify();
    }

    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cursor) = self.layouts.hit(event.position) else {
            return;
        };
        let cursor = cursor.clamp(&self.document);
        self.focus.focus(window, cx);
        self.pointer = event.position;
        self.dragging = true;
        self.selection = Some(if event.modifiers.shift {
            self.selection
                .unwrap_or_else(|| Selection::at(cursor))
                .extend_to(cursor)
        } else if event.click_count >= 3 {
            Selection::new(
                Cursor {
                    offset: 0,
                    ..cursor
                },
                cursor.end(&self.document),
            )
        } else if event.click_count == 2 {
            word(&self.document, cursor)
        } else {
            Selection::at(cursor)
        });
        cx.notify();
    }

    fn moved(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let previous = (self.pointer, self.selection);
        self.pointer = position;
        if let Some(cursor) = self.layouts.hit(position) {
            self.selection = self
                .selection
                .map(|selection| selection.extend_to(cursor.clamp(&self.document)));
        }
        if previous != (self.pointer, self.selection) {
            cx.notify();
        }
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        self.dragging = false;
        cx.notify();
    }

    fn autoscroll(&mut self, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let Some(scroll) = &self.scroll else {
            return;
        };
        let bounds = scroll.bounds();
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
        // Move the nearest supplied viewport directly. Synthetic wheel events
        // would bubble into conversation and window scrollers at an inner edge.
        let scrolled = scroll.scroll_by(delta);
        let selected = self.selection;
        // Even at the final scroll offset, use this frame's newly mounted
        // layouts so the last visible line participates in the selection.
        self.moved(self.pointer, cx);
        if scrolled && selected == self.selection {
            cx.notify();
        }
    }
}

impl Focusable for State {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for State {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.weak_entity();
        let listener = self.on_link.clone();
        let on_link: OnLink = Rc::new(move |url, window, cx| {
            let allowed = entity
                .update(cx, |state, _| {
                    state
                        .selection
                        .is_none_or(|selection| selection.is_collapsed())
                })
                .unwrap_or(false);
            if allowed {
                if let Some(listener) = &listener {
                    listener(url, window, cx);
                } else {
                    cx.open_url(url);
                }
            }
        });
        // A virtual list holds its layout borrow while rendering its children.
        // Query and mutate the supplied viewport only after that paint ends.
        if self.dragging && self.scroll.is_some() {
            let entity = cx.weak_entity();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |state, cx| state.autoscroll(cx));
            });
        }
        let listener = self.dragging.then(|| {
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
        div()
            .id("markdown-surface")
            .min_w_0()
            .w_full()
            .key_context("SailryMarkdown")
            .track_focus(&self.focus)
            .cursor_text()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|state, _, _, cx| state.release(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|state, _, _, cx| state.release(cx)),
            )
            .on_action(cx.listener(|state, _: &Copy, _, cx| {
                let text = state.selected_text();
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|state, _: &SelectAll, _, cx| {
                state.select_all(cx);
            }))
            .children(listener)
            .child(render_with(
                &self.document,
                Editing {
                    selection: self.selection,
                    caret_on: false,
                    layouts: Some(&self.layouts),
                    on_link: Some(on_link),
                    on_image: self.on_image.clone(),
                    on_file: self.on_file.clone(),
                    ..Editing::default()
                },
                window,
                cx,
            ))
    }
}

#[derive(IntoElement)]
pub struct View {
    state: Option<Entity<State>>,
    source: Option<(ElementId, SharedString)>,
    on_link: Option<OnLink>,
    on_image: Option<OnImage>,
    on_file: Option<OnFile>,
    scroll: Option<Viewport>,
}

impl View {
    pub fn new(state: &Entity<State>) -> Self {
        Self {
            state: Some(state.clone()),
            source: None,
            on_link: None,
            on_image: None,
            on_file: None,
            scroll: None,
        }
    }
    /// Retain selection and parsed content at this element's stable identity.
    pub fn markdown(id: impl Into<ElementId>, source: impl Into<SharedString>) -> Self {
        Self {
            state: None,
            source: Some((id.into(), source.into())),
            on_link: None,
            on_image: None,
            on_file: None,
            scroll: None,
        }
    }
    pub fn on_link_click(
        mut self,
        listener: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_link = Some(Rc::new(listener));
        self
    }
    pub(crate) fn on_image(
        mut self,
        render: impl Fn(&str, &App) -> gpui_kit::AnyElement + 'static,
    ) -> Self {
        self.on_image = Some(Rc::new(render));
        self
    }
    pub(crate) fn on_file(
        mut self,
        render: impl Fn(usize, &str, &str, &App) -> Option<gpui_kit::AnyElement> + 'static,
    ) -> Self {
        self.on_file = Some(Rc::new(render));
        self
    }
    /// The containing vertical viewport, retained by its existing owner.
    pub fn scroll_handle(mut self, handle: ScrollHandle) -> Self {
        self.scroll = Some(Viewport::Scroll(handle));
        self
    }
    pub fn list_state(mut self, state: ListState) -> Self {
        self.scroll = Some(Viewport::List(state));
        self
    }
}

impl RenderOnce for View {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = if let Some(state) = self.state {
            state
        } else {
            let (id, source) = self
                .source
                .expect("Markdown view has a source or retained state");
            let state = window.use_keyed_state(id, cx, |_, cx| State::new(source.clone(), cx));
            state.update(cx, |state, cx| state.set_source(source, cx));
            state
        };
        state.update(cx, |state, _| {
            if state.on_image.is_some() != self.on_image.is_some() {
                let doc = parse_ranges(state.source.as_ref()).doc;
                state.document = Rc::new(if self.on_image.is_some() {
                    super::images::expand(doc)
                } else {
                    doc
                });
                state.selection = None;
            }
            state.on_file = self.on_file;
            state.on_image = self.on_image;
            state.on_link = self.on_link;
            state.scroll = self.scroll;
        });
        state
    }
}

fn word(doc: &Doc, cursor: Cursor) -> Selection {
    let Some(text) = doc
        .blocks
        .get(cursor.block)
        .and_then(|block| block.text_at(cursor.part))
    else {
        return Selection::at(cursor);
    };
    let range = text
        .text
        .split_word_bound_indices()
        .find_map(|(start, word)| {
            let end = start + word.len();
            (start <= cursor.offset && cursor.offset < end).then_some(start..end)
        })
        .unwrap_or(cursor.offset..cursor.offset);
    Selection::new(
        Cursor {
            offset: range.start,
            ..cursor
        },
        Cursor {
            offset: range.end,
            ..cursor
        },
    )
    .clamp(doc)
}

fn edge_delta(pointer: f32, top: f32, bottom: f32) -> f32 {
    let margin = 24_f32.min((bottom - top) / 2.);
    if pointer < top + margin {
        -((top + margin - pointer) * 0.5).min(24.)
    } else if pointer > bottom - margin {
        ((pointer - bottom + margin) * 0.5).min(24.)
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::markdown::{Part, parse};
    use core::prelude::v1::test;
    use gpui_kit as gpui;
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::Cell;

    #[test]
    fn selects_unicode_words_and_emoji() {
        let doc = parse("Hello é 👩‍💻 world");
        let cursor = Cursor::new(0, Part::Body, "Hello é ".len());
        assert_eq!(
            super::super::selectable::copied(&doc, word(&doc, cursor)),
            "👩‍💻"
        );
    }

    #[test]
    fn scrolls_only_at_viewport_edges() {
        assert_eq!(edge_delta(150., 100., 200.), 0.);
        assert!(edge_delta(105., 100., 200.) < 0.);
        assert!(edge_delta(195., 100., 200.) > 0.);
        assert_eq!(edge_delta(500., 100., 200.), 24.);
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn selects_virtualized_rows(cx: &mut TestAppContext) {
        struct Fixture {
            state: Entity<State>,
            list: ListState,
        }
        impl Render for Fixture {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let state = self.state.clone();
                let list = self.list.clone();
                gpui_kit::list(self.list.clone(), move |_, _, _| {
                    View::new(&state)
                        .list_state(list.clone())
                        .into_any_element()
                })
                .w(px(360.))
                .h(px(180.))
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let state = cx.new(|cx| State::new("paragraph\n\n".repeat(40), cx));
        let list = ListState::new(1, gpui_kit::ListAlignment::Top, px(0.));
        let (_, visual) = cx.add_window_view(|window, cx| {
            let fixture = cx.new(|_| Fixture {
                state: state.clone(),
                list: list.clone(),
            });
            Root::new(fixture, window, cx)
        });
        draw(visual);
        visual.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.dragging = true;
                state.selection = Some(Selection::at(Cursor::new(0, Part::Body, 0)));
                state.pointer = list.viewport_bounds().bottom_right() + point(px(30.), px(30.));
                cx.notify();
            });
        });
        // Rendering a dragging child must not borrow the containing list.
        draw(visual);
        visual.update(|window, cx| {
            window.simulate_next_frame(cx);
            assert!(list.scroll_px_offset_for_scrollbar().y < px(0.));
            state.update(cx, |state, cx| state.release(cx));
            list.scroll_to_end();
        });
        draw(visual);
        visual.update(|_, cx| {
            let before = list.scroll_px_offset_for_scrollbar();
            state.update(cx, |state, cx| {
                state.dragging = true;
                state.selection = Some(Selection::at(Cursor::new(0, Part::Body, 0)));
                state.pointer = list.viewport_bounds().bottom_right() + point(px(30.), px(30.));
                state.autoscroll(cx);
                state.dragging = false;
                assert_eq!(state.selection.unwrap().head.block, 39);
            });
            // At the final offset, refresh the endpoint using the last layout.
            assert_eq!(list.scroll_px_offset_for_scrollbar(), before);
        });
    }

    #[gpui::test]
    fn distinguishes_links_from_selection(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let clicks = Rc::new(Cell::new(0));
        let count = clicks.clone();
        let mut state = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let markdown = cx.new(|cx| {
                let mut state = State::new("[Read](https://example.com) more 中文", cx);
                state.on_link = Some(Rc::new(move |link, _, _| {
                    assert_eq!(link.as_ref(), "https://example.com");
                    count.set(count.get() + 1);
                }));
                state
            });
            state = Some(markdown.clone());
            Root::new(markdown, window, cx)
        });
        let state = state.unwrap();
        draw(visual);
        let (start, end) = visual.update(|_, cx| {
            let layouts = &state.read(cx).layouts;
            let (start, height) = layouts.position(Cursor::new(0, Part::Body, 0)).unwrap();
            let (end, _) = layouts
                .position(Cursor::new(0, Part::Body, "Read more ".len()))
                .unwrap();
            (
                start + point(px(2.), height / 2.),
                end + point(px(1.), height / 2.),
            )
        });
        visual.simulate_click(start, Modifiers::none());
        draw(visual);
        assert_eq!(clicks.get(), 1);
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        draw(visual);
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        draw(visual);
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        draw(visual);
        assert_eq!(clicks.get(), 1);
        visual.update(|_, cx| assert!(state.read(cx).selected_text().contains("Read more")));
    }
    #[gpui::test]
    fn padded_code_and_link_icons_keep_selection(cx: &mut TestAppContext) {
        struct Fixture(Entity<State>, Rc<Cell<usize>>);
        impl Render for Fixture {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicks = self.1.clone();
                div()
                    .w(px(240.))
                    .child(View::new(&self.0).on_link_click(move |url, _, _| {
                        assert_eq!(url.as_ref(), "file.rs");
                        clicks.set(clicks.get() + 1);
                    }))
            }
        }
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let source =
            "[中文链接](file.rs)，`read_file`；更多内容 `long_command_name_with_arguments` 结束";
        let state = cx.new(|cx| State::new(source, cx));
        let count = Rc::new(Cell::new(0));
        let owner = state.clone();
        let (_, visual) = cx.add_window_view(|window, cx| {
            Root::new(cx.new(|_| Fixture(owner, count.clone())), window, cx)
        });
        draw(visual);
        let (icon, start, end) = visual.update(|_, cx| {
            let state = state.read(cx);
            let text = &state.document.blocks[0].text_at(Part::Body).unwrap().text;
            let offset = text.find("read_file").unwrap();
            let layouts = &state.layouts;
            let (link, height) = layouts.position(Cursor::new(0, Part::Body, 0)).unwrap();
            let (start, _) = layouts
                .position(Cursor::new(0, Part::Body, offset))
                .unwrap();
            let (end, _) = layouts
                .position(Cursor::new(0, Part::Body, offset + 9))
                .unwrap();
            assert!(start.x < end.x);
            let inset = start + point(px(-1.), height / 2.);
            assert_eq!(layouts.hit(inset).unwrap().offset, offset);
            (
                link + point(px(-7.), height / 2.),
                start + point(px(0.), height / 2.),
                end + point(px(0.), height / 2.),
            )
        });
        visual.simulate_click(icon, Modifiers::none());
        draw(visual);
        assert_eq!(count.get(), 1);
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        draw(visual);
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        draw(visual);
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        draw(visual);
        visual.update(|window, cx| {
            assert_eq!(state.read(cx).selected_text(), "read_file");
            assert_eq!(state.read(cx).source(), source);
            state.focus_handle(cx).focus(window, cx);
        });
        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a cmd-c"
        } else {
            "ctrl-a ctrl-c"
        });
        visual.update(|_, cx| {
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().unwrap(),
                "中文链接，read_file；更多内容 long_command_name_with_arguments 结束"
            );
        });
    }
}
