//! Five-line plain-text previews retain Kit selection, links and full source.
use crate::{content::selection::Excerpt, tr};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    base::{SelectableText, TextSelection, TextSelectionHandle},
    component::{
        button::{Button, ButtonVariants},
        input::{InlineToken, InputContent, Textarea, TextareaState},
        *,
    },
    *,
};
use std::{ops::Range, rc::Rc};

#[cfg(test)]
mod tests;
mod width;

type Resize = Rc<dyn Fn(&mut Window, &mut App)>;
type Selected = Rc<dyn Fn(Excerpt, &mut Window, &mut App)>;
type OnLink = Rc<dyn Fn(&Link, &mut Window, &mut App)>;

#[derive(Clone)]
pub(crate) struct Link {
    pub(crate) range: Range<usize>,
    pub(crate) id: SharedString,
}

pub(crate) struct State {
    text: SharedString,
    selection: TextSelectionHandle,
    height: Pixels,
    expanded: bool,
    selecting: bool,
    origin: Point<Pixels>,
    multiple: bool,
    references: Option<Entity<TextareaState>>,
    reference_width: Option<Pixels>,
    reference_epoch: u64,
}

impl State {
    pub(crate) fn new(text: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let text = text.into();
        Self {
            selection: TextSelectionHandle::new(text.to_string(), cx),
            text,
            height: px(0.),
            expanded: false,
            selecting: false,
            origin: Point::default(),
            multiple: false,
            references: None,
            reference_width: None,
            reference_epoch: 0,
        }
    }

    pub(crate) fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        if self.text != text {
            self.text = text;
            self.height = px(0.);
            self.expanded = false;
            self.references = None;
            self.reference_width = None;
            self.reference_epoch += 1;
            self.selection
                .set_fallback_copy_text(self.text.to_string(), cx);
        }
    }
}

#[derive(IntoElement)]
pub(crate) struct View {
    state: Entity<State>,
    id: SharedString,
    text_size: Pixels,
    on_resize: Resize,
    on_selection: Option<Selected>,
    links: Vec<Link>,
    on_link: Option<OnLink>,
    adaptive_width: bool,
}

impl View {
    pub(crate) fn new(
        state: &Entity<State>,
        id: impl Into<SharedString>,
        text_size: Pixels,
    ) -> Self {
        Self {
            state: state.clone(),
            id: id.into(),
            text_size,
            on_resize: Rc::new(|window, _| window.refresh()),
            on_selection: None,
            links: vec![],
            on_link: None,
            adaptive_width: false,
        }
    }
    pub(crate) fn on_resize(mut self, listener: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_resize = Rc::new(listener);
        self
    }
    pub(crate) fn on_selection(
        mut self,
        listener: impl Fn(Excerpt, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_selection = Some(Rc::new(listener));
        self
    }
    pub(crate) fn links(
        mut self,
        links: Vec<Link>,
        listener: impl Fn(&Link, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.links = links;
        self.on_link = Some(Rc::new(listener));
        self
    }

    pub(crate) fn adaptive_width(mut self) -> Self {
        self.adaptive_width = true;
        self
    }
}

impl RenderOnce for View {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let line_height = self.text_size * 1.6;
        let limit = line_height * 5.;
        let overflow = state.height > limit + px(0.5);
        let expanded = state.expanded;
        let selection = state.selection.clone();
        let source = state.text.clone();
        let measurements = Rc::new(std::cell::RefCell::new(width::Measurement::default()));
        let reference_input = if self.links.is_empty() {
            None
        } else {
            let mut content = InputContent::new(source.clone());
            for link in &self.links {
                content = content
                    .with_token(
                        link.range.clone(),
                        InlineToken::new(link.id.clone(), source[link.range.clone()].to_owned()),
                    )
                    .expect("captured reference matches the prompt source");
            }
            Some(self.state.update(cx, |state, cx| {
                let input = state
                    .references
                    .get_or_insert_with(|| {
                        cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, usize::MAX))
                    })
                    .clone();
                if input.read(cx).content() != content {
                    state.reference_width = None;
                    input.update(cx, |input, cx| input.set_value(content, window, cx));
                }
                state.reference_epoch += 1;
                input
            }))
        };
        let adaptive = self.adaptive_width && reference_input.is_some();
        let (reference_width, reference_epoch) = {
            let state = self.state.read(cx);
            (state.reference_width, state.reference_epoch)
        };
        let text = if let Some(input) = &reference_input {
            let listener = self.on_link;
            let measured = measurements.clone();
            let source = source.clone();
            let links = self.links.clone();
            Textarea::new(input)
                .appearance(false)
                .bordered(false)
                .readonly(true)
                .when(adaptive, |input| {
                    input.token(move |context, window, cx| {
                        measured
                            .borrow_mut()
                            .token(context, &source, &links, window, cx);
                        gpui_kit::component::input::InputToken::new(context)
                    })
                })
                .on_token_click(move |event, window, cx| {
                    if let Some(link) = self
                        .links
                        .iter()
                        .find(|link| link.range == event.range() && &link.id == event.token().id())
                        && let Some(listener) = &listener
                    {
                        listener(link, window, cx);
                    }
                })
                .into_any_element()
        } else {
            SelectableText::with_handle(format!("{}-prompt", self.id), selection.clone(), source)
                .into_any_element()
        };
        let measuring = self.state.clone();
        let resize = self.on_resize.clone();
        let width_resize = resize.clone();
        let text_id = self.id.clone();
        let viewport_id = self.id.clone();
        let button_id = self.id.clone();
        let selected = self.on_selection;
        let releasing = self.state.clone();
        let pressing = self.state.clone();
        let released = move |event: &MouseUpEvent, window: &mut Window, cx: &mut App| {
            let (selecting, origin, multiple) = releasing.update(cx, |state, _| {
                (
                    std::mem::take(&mut state.selecting),
                    state.origin,
                    state.multiple,
                )
            });
            if !selecting {
                return;
            }
            let Some(listener) = selected.clone() else {
                return;
            };
            if let Some(input) = &reference_input {
                let delta = event.position - origin;
                if !multiple && delta.x.abs() + delta.y.abs() <= px(3.) {
                    return;
                }
                let text = input.read(cx).selected_value().to_string();
                if !text.trim().is_empty() {
                    let end = event.position;
                    let anchor = Bounds::from_corners(
                        point(origin.x.min(end.x), origin.y.min(end.y)),
                        point(
                            origin.x.max(end.x) + px(1.),
                            origin.y.max(end.y) + line_height,
                        ),
                    );
                    listener(Excerpt { text, anchor }, window, cx);
                }
                return;
            }
            let selection = selection.clone();
            // Kit projects selected copy text while painting the finished drag.
            window.on_next_frame(move |window, cx| {
                window.defer(cx, move |window, cx| {
                    let Some(snapshot) = selection.snapshot(cx) else {
                        return;
                    };
                    if snapshot.is_selecting()
                        || snapshot.anchor().entity_id() != Some(selection.entity_id())
                    {
                        return;
                    }
                    let Some(points) = snapshot.window_points() else {
                        return;
                    };
                    let text = TextSelection::selected_text(window, cx);
                    if !text.trim().is_empty() {
                        let start = points.anchor();
                        let end = points.cursor();
                        let anchor = Bounds::from_corners(
                            point(start.x.min(end.x), start.y.min(end.y)),
                            point(
                                start.x.max(end.x) + px(1.),
                                start.y.max(end.y) + line_height,
                            ),
                        );
                        listener(Excerpt { text, anchor }, window, cx);
                    }
                });
            });
            window.refresh();
        };
        let released_out = released.clone();
        v_flex()
            .id(("prompt", self.state.entity_id()))
            .test_support()
            .min_w_0()
            // Kit's Textarea fills its parent but has no intrinsic width. Only
            // sent reference prompts opt into native text/token measurement;
            // the existing Bubble still owns its surface and maximum width.
            .when(adaptive, |body| {
                body.w(
                    reference_width.unwrap_or(px(crate::conversation::layout::USER_MESSAGE_WIDTH))
                )
                .max_w_full()
            })
            .text_size(self.text_size)
            .line_height(relative(1.6))
            .child(
                div()
                    .relative()
                    .min_w_0()
                    .when(!expanded, |viewport| {
                        viewport.max_h(limit).overflow_hidden()
                    })
                    .debug_selector(move || format!("message-preview-{viewport_id}"))
                    .child(
                        div()
                            .id(("prompt-text", self.state.entity_id()))
                            .flex_none()
                            .min_w_0()
                            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                pressing.update(cx, |state, _| {
                                    state.selecting = true;
                                    state.origin = event.position;
                                    state.multiple = event.click_count > 1;
                                });
                            })
                            .on_mouse_up(MouseButton::Left, released)
                            .on_mouse_up_out(MouseButton::Left, released_out)
                            .debug_selector(move || format!("message-text-{text_id}"))
                            .child(text)
                            .on_prepaint(move |bounds, window, cx| {
                                if adaptive {
                                    let measurements = measurements.clone();
                                    let measuring = measuring.clone();
                                    let resize = width_resize.clone();
                                    window.defer(cx, move |window, cx| {
                                        let Some(width) =
                                            measurements.borrow().width(bounds.size.width)
                                        else {
                                            return;
                                        };
                                        let changed = measuring.update(cx, |state, cx| {
                                            if state.reference_epoch != reference_epoch
                                                || state.reference_width.is_some_and(|previous| {
                                                    (previous - width).abs() <= px(0.5)
                                                })
                                            {
                                                return false;
                                            }
                                            state.reference_width = Some(width);
                                            cx.notify();
                                            true
                                        });
                                        if changed {
                                            resize(window, cx);
                                        }
                                    });
                                }
                                if (measuring.read(cx).height - bounds.size.height).abs() <= px(0.5)
                                {
                                    return;
                                }
                                let measuring = measuring.clone();
                                let resize = resize.clone();
                                window.defer(cx, move |window, cx| {
                                    measuring
                                        .update(cx, |state, _| state.height = bounds.size.height);
                                    resize(window, cx);
                                });
                            }),
                    ),
            )
            .when(overflow, |body| {
                body.child(
                    h_flex().w_full().justify_center().pt_1().child(
                        Button::new(("prompt-expand", self.state.entity_id()))
                            .ghost()
                            .xsmall()
                            .icon(if expanded {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .accessibility_label(tr(if expanded {
                                "message_collapse"
                            } else {
                                "message_expand"
                            }))
                            .tooltip(tr(if expanded {
                                "message_collapse"
                            } else {
                                "message_expand"
                            }))
                            .debug_selector(move || format!("message-expand-{button_id}"))
                            .on_click(move |_, window, cx| {
                                self.state
                                    .update(cx, |state, _| state.expanded = !state.expanded);
                                (self.on_resize)(window, cx);
                            }),
                    ),
                )
            })
    }
}
