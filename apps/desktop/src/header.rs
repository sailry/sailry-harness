//! Shared workspace and panel headers; feature views supply their own content.
use crate::preview::HEADER_HEIGHT;
use gpui_kit::{
    component::{ActiveTheme, TitleBar, h_flex},
    *,
};

#[derive(IntoElement)]
pub(crate) struct Header {
    id: SharedString,
    content: Div,
    controls: bool,
    draggable: bool,
}

impl Header {
    pub(crate) fn new(id: impl Into<SharedString>, cx: &App) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            content: h_flex()
                .debug_selector(move || id.to_string())
                .h(px(HEADER_HEIGHT))
                .flex_shrink_0()
                .min_w_0()
                .px_3()
                .gap_2()
                .border_b_1()
                .border_color(cx.theme().border),
            controls: false,
            draggable: true,
        }
    }

    pub(crate) fn workspace(id: &'static str, cx: &App) -> Self {
        Self {
            controls: true,
            ..Self::new(id, cx).bordered(false)
        }
    }

    pub(crate) fn bordered(mut self, bordered: bool) -> Self {
        self.content = if bordered {
            self.content.border_b_1()
        } else {
            self.content.border_b_0()
        };
        self
    }

    pub(crate) fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }
}

impl Styled for Header {
    fn style(&mut self) -> &mut StyleRefinement {
        self.content.style()
    }
}

impl ParentElement for Header {
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(children);
    }
}

#[derive(Default)]
struct Drag {
    origin: Option<Point<Pixels>>,
}

impl RenderOnce for Header {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.controls {
            return TitleBar::new()
                .h(px(HEADER_HEIGHT))
                .pl_0()
                .bg(transparent_black())
                .border_b_0()
                // Keep the flexible title bounded so trailing controls remain visible.
                .child(
                    div()
                        .w_0()
                        .flex_1()
                        .overflow_hidden()
                        .child(self.content.w_full()),
                )
                .into_any_element();
        }
        if !self.draggable {
            return self.content.into_any_element();
        }
        // Kit b79f4ce's TitleBar always adds window controls. Panel headers need
        // only its native drag interaction, with state local to each header.
        // Kit controls retain their own focus and stop mouse-down propagation.
        let state = window.use_keyed_state(self.id, cx, |_, _| Drag::default());
        self.content
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&state, |state, event: &MouseDownEvent, window, _| {
                    state.origin = (event.click_count == 1).then_some(event.position);
                    if event.click_count == 2 {
                        if cfg!(target_os = "macos") {
                            window.titlebar_double_click();
                        } else if cfg!(target_os = "linux") {
                            window.zoom_window();
                        }
                    }
                }),
            )
            .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| state.origin = None))
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&state, |state, _, _, _| state.origin = None),
            )
            .on_mouse_move(window.listener_for(
                &state,
                |state, event: &MouseMoveEvent, window, _| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        state.origin = None;
                    }
                    if state.origin.is_some_and(|origin| origin != event.position) {
                        state.origin = None;
                        window.start_window_move();
                    }
                },
            ))
            .into_any_element()
    }
}

pub(crate) fn separator(cx: &App) -> Div {
    div()
        .w_px()
        .h_4()
        .mx_1()
        .flex_shrink_0()
        .bg(cx.theme().border)
}
