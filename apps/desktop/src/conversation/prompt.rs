//! Five-line plain-text previews retain Kit selection, links and full source.
use crate::{content::selection::Excerpt, tr};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    base::{LinkSpan, SelectableText, TextSelection, TextSelectionHandle},
    component::{
        button::{Button, ButtonVariants},
        *,
    },
    *,
};
use std::rc::Rc;

type Resize = Rc<dyn Fn(&mut Window, &mut App)>;
type Selected = Rc<dyn Fn(Excerpt, &mut Window, &mut App)>;
type Link = Rc<dyn Fn(&LinkSpan, &mut Window, &mut App)>;

pub(crate) struct State {
    text: SharedString,
    selection: TextSelectionHandle,
    height: Pixels,
    expanded: bool,
    selecting: bool,
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
        }
    }

    pub(crate) fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        if self.text != text {
            self.text = text;
            self.height = px(0.);
            self.expanded = false;
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
    links: Vec<LinkSpan>,
    on_link: Option<Link>,
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
        links: Vec<LinkSpan>,
        listener: impl Fn(&LinkSpan, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.links = links;
        self.on_link = Some(Rc::new(listener));
        self
    }
}

impl RenderOnce for View {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.read(cx);
        let line_height = self.text_size * 1.6;
        let limit = line_height * 5.;
        let overflow = state.height > limit + px(0.5);
        let expanded = state.expanded;
        let selection = state.selection.clone();
        let mut text = SelectableText::with_handle(
            format!("{}-prompt", self.id),
            selection.clone(),
            state.text.clone(),
        );
        if let Some(listener) = self.on_link {
            text = text.links(self.links, cx.theme().link, move |link, window, cx| {
                listener(link, window, cx)
            });
        }
        let measuring = self.state.clone();
        let resize = self.on_resize.clone();
        let text_id = self.id.clone();
        let viewport_id = self.id.clone();
        let button_id = self.id.clone();
        let selected = self.on_selection;
        let releasing = self.state.clone();
        let pressing = self.state.clone();
        let released = move |_: &MouseUpEvent, window: &mut Window, cx: &mut App| {
            if !releasing.update(cx, |state, _| std::mem::take(&mut state.selecting)) {
                return;
            }
            let Some(listener) = selected.clone() else {
                return;
            };
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
            .min_w_0()
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
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                pressing.update(cx, |state, _| state.selecting = true);
                            })
                            .on_mouse_up(MouseButton::Left, released)
                            .on_mouse_up_out(MouseButton::Left, released_out)
                            .debug_selector(move || format!("message-text-{text_id}"))
                            .child(text)
                            .on_prepaint(move |bounds, window, cx| {
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
