//! A retained, selectable surface over the Bezel presentation model.
//!
//! Source bytes remain authoritative. Parsing runs only when source changes;
//! pointer moves, theme changes and streaming frames reuse the same document.
use super::pointer::{edge_delta, word};
use std::rc::Rc;

use gpui_kit::{
    App, ClipboardItem, Context, DispatchPhase, ElementId, Entity, FocusHandle, Focusable, Global,
    IntoElement, KeyBinding, ListState, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    Render, RenderOnce, ScrollHandle, SharedString, Window, canvas, div, point, prelude::*, px,
};

use super::render::{OnFile, OnImage, OnLink};
use super::{BlockLayouts, Cursor, Doc, Editing, Selection, with_ranges, with_selection};
use crate::selection::Excerpt;

type OnSelection = Rc<dyn Fn(Excerpt, &mut Window, &mut App)>;

gpui_kit::actions!(sailry_markdown, [Copy, SelectAll]);
struct Bindings;
impl Global for Bindings {}

pub struct State {
    source: SharedString,
    pub(super) document: Rc<Doc>,
    pub(super) visuals: super::visuals::state::Cache,
    layouts: BlockLayouts,
    selection: Option<Selection>,
    dragging: bool,
    pointer: Point<Pixels>,
    focus: FocusHandle,
    on_link: Option<OnLink>,
    on_image: Option<OnImage>,
    on_file: Option<OnFile>,
    on_selection: Option<OnSelection>,
    hover_links: bool,
    underline_links: bool,
    hovered_link: Option<Cursor>,
    scroll: Option<Viewport>,
    pub(super) typography: Option<super::typography::Typography>,
    pub(super) foreground: Option<gpui_kit::Hsla>,
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
        let parsed = with_ranges(source.as_ref());
        Self {
            document: Rc::new(parsed.doc),
            visuals: super::visuals::state::Cache::default(),
            source,
            layouts: BlockLayouts::default(),
            selection: None,
            dragging: false,
            pointer: point(px(0.), px(0.)),
            focus: cx.focus_handle(),
            on_link: None,
            on_image: None,
            on_file: None,
            on_selection: None,
            hover_links: false,
            underline_links: true,
            hovered_link: None,
            scroll: None,
            typography: None,
            foreground: None,
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
        let parsed = with_ranges(source.as_ref());
        self.document = Rc::new(if self.on_image.is_some() {
            super::images::expand(parsed.doc)
        } else {
            parsed.doc
        });
        self.source = source;
        self.hovered_link = None;
        self.selection = self
            .selection
            .map(|selection| selection.clamp(&self.document));
        cx.notify();
    }

    fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .document
            .blocks
            .iter()
            .enumerate()
            .any(|(index, block)| {
                block.parts().iter().any(|part| {
                    block.text_at(*part).is_some_and(|text| {
                        text.marks.iter().any(|mark| mark.mark == super::Mark::Math)
                    })
                }) && self
                    .layouts
                    .block_bounds(index)
                    .is_some_and(|bounds| bounds.contains(&event.position))
            })
        {
            self.selection = None;
            self.dragging = false;
            cx.notify();
            return;
        }
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

    fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.dragging) {
            self.publish_selection(window, cx);
        }
        cx.notify();
    }

    fn publish_selection(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(listener) = &self.on_selection else {
            return;
        };
        let Some(selection) = self.selection.filter(|selection| !selection.is_collapsed()) else {
            return;
        };
        let (start, end) = selection.ordered();
        let (Some((start, _)), Some((end, height))) =
            (self.layouts.position(start), self.layouts.position(end))
        else {
            return;
        };
        let anchor = gpui_kit::Bounds::from_corners(
            point(start.x.min(end.x), start.y.min(end.y)),
            point(start.x.max(end.x) + px(1.), start.y.max(end.y) + height),
        );
        let text = self.selected_text();
        if !text.trim().is_empty() {
            listener(Excerpt { text, anchor }, window, cx);
        }
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
        super::visuals::state::Cache::prepare(self, window, cx);
        let visuals = self.visuals.images();
        let math_entity = cx.weak_entity();
        let on_math_link: super::render::OnMathLink =
            std::sync::Arc::new(move |url, window, cx| {
                let _ = math_entity.update(cx, |state, cx| {
                    if let Some(listener) = &state.on_link {
                        listener(url, window, cx);
                    } else {
                        cx.open_url(url);
                    }
                });
            });
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
            .on_hover(cx.listener(|state, hovered: &bool, _, cx| {
                // InteractiveText reports indices only inside its hitbox.
                if !hovered && state.hovered_link.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|state, _, window, cx| state.release(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|state, _, window, cx| state.release(window, cx)),
            )
            .on_action(cx.listener(|state, _: &Copy, _, cx| {
                let text = state.selected_text();
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|state, _: &SelectAll, window, cx| {
                state.select_all(cx);
                state.publish_selection(window, cx);
            }))
            .children(listener)
            .child(with_selection(
                &self.document,
                Editing {
                    selection: self.selection,
                    caret_on: false,
                    layouts: Some(&self.layouts),
                    on_link: Some(on_link),
                    hovered_link: self.hovered_link,
                    underline_links: self.underline_links,
                    on_link_hover: self.hover_links.then(|| {
                        let owner = cx.weak_entity();
                        Rc::new(
                            move |at: Cursor,
                                  index: Option<usize>,
                                  _: &mut Window,
                                  cx: &mut App| {
                                _ = owner.update(cx, |state, cx| {
                                    let next =
                                        index.map(|offset| Cursor::new(at.block, at.part, offset));
                                    if next.is_none()
                                        && state.hovered_link.is_some_and(|current| {
                                            current.block != at.block || current.part != at.part
                                        })
                                    {
                                        return;
                                    }
                                    if state.hovered_link != next {
                                        state.hovered_link = next;
                                        cx.notify();
                                    }
                                });
                            },
                        ) as super::render::OnLinkHover
                    }),
                    on_image: self.on_image.clone(),
                    on_file: self.on_file.clone(),
                    typography: self.typography,
                    foreground: self.foreground,
                    visuals: Some(&visuals),
                    on_math_link: Some(on_math_link),
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
    on_selection: Option<OnSelection>,
    scroll: Option<Viewport>,
    typography: Option<super::typography::Typography>,
    foreground: Option<gpui_kit::Hsla>,
    hover_links: bool,
    underline_links: bool,
}

impl View {
    pub fn new(state: &Entity<State>) -> Self {
        Self {
            state: Some(state.clone()),
            source: None,
            on_link: None,
            on_image: None,
            on_file: None,
            on_selection: None,
            scroll: None,
            typography: None,
            foreground: None,
            hover_links: false,
            underline_links: true,
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
            on_selection: None,
            scroll: None,
            typography: None,
            foreground: None,
            hover_links: false,
            underline_links: true,
        }
    }
    pub fn on_link_click(
        mut self,
        listener: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_link = Some(Rc::new(listener));
        self
    }
    pub fn on_image(
        mut self,
        render: impl Fn(&str, &[&str], usize, &App) -> gpui_kit::AnyElement + 'static,
    ) -> Self {
        self.on_image = Some(Rc::new(render));
        self
    }
    pub fn on_file(
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

    pub fn foreground(mut self, color: Option<gpui_kit::Hsla>) -> Self {
        self.foreground = color;
        self
    }

    pub fn link_hover_underline(mut self, enabled: bool) -> Self {
        self.hover_links = enabled;
        self
    }

    pub fn link_underline(mut self, enabled: bool) -> Self {
        self.underline_links = enabled;
        self
    }

    pub fn on_selection(
        mut self,
        listener: impl Fn(Excerpt, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_selection = Some(Rc::new(listener));
        self
    }

    pub fn typography(mut self, typography: super::typography::Typography) -> Self {
        self.typography = Some(typography);
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
        state.update(cx, |state, cx| {
            if state.on_image.is_some() != self.on_image.is_some() {
                let doc = with_ranges(state.source.as_ref()).doc;
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
            state.on_selection = self.on_selection;
            state.scroll = self.scroll;
            if state.underline_links != self.underline_links {
                state.underline_links = self.underline_links;
                cx.notify();
            }
            if state.hover_links != self.hover_links {
                state.hover_links = self.hover_links;
                state.hovered_link = None;
                cx.notify();
            }
            if state.foreground != self.foreground {
                state.foreground = self.foreground;
                cx.notify();
            }
            if state.typography != self.typography {
                state.typography = self.typography;
                cx.notify();
            }
        });
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{Part, parse};
    use core::prelude::v1::test;

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
}
