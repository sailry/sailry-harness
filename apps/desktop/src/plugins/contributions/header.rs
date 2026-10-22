//! Kit supplies the popover surface/focus/dismissal. This adapter adds hover-to-open
//! and click-to-hold together, which the pinned HoverCard does not expose.
use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover, *,
};
use std::time::Duration;

pub(crate) struct Popover {
    entry: Entry,
    panel: Entity<Panel>,
    open: bool,
    held: bool,
    hovered: bool,
    focus: FocusHandle,
    timer: Option<Task<()>>,
}

impl Popover {
    pub(crate) fn new(
        entry: Entry,
        panel: Entity<Panel>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        Self {
            entry,
            panel,
            open: false,
            held: false,
            hovered: false,
            focus,
            timer: None,
        }
    }

    pub(crate) fn update(&mut self, entry: Entry, panel: Entity<Panel>, cx: &mut Context<Self>) {
        if self.entry.state != entry.state
            || self.entry.declaration != entry.declaration
            || self.panel != panel
        {
            self.entry = entry;
            self.panel = panel;
            cx.notify();
        }
    }

    fn hover(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hovered = hovered;
        self.timer = None;
        if self.held || !self.entry.state.enabled {
            return;
        }
        let Some(hover) = &self.entry.declaration.hover else {
            return;
        };
        let delay = if hovered {
            hover.open_ms
        } else {
            hover.close_ms
        };
        self.timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay.into()))
                .await;
            let _ = this.update_in(cx, |view, _, cx| {
                if !view.held {
                    view.open = view.hovered;
                    cx.notify();
                }
            });
        }));
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.timer = None;
        self.open = false;
        self.held = false;
        cx.notify();
    }
}

impl Render for Popover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = px(f32::from(self.entry.declaration.width.unwrap_or(480)))
            .min((window.viewport_size().width - px(32.)).max(px(100.)));
        let height = (window.viewport_size().height - px(110.)).max(px(100.));
        let button = Button::new("header-contribution-trigger")
            .ghost()
            .small()
            .icon(self.entry.icon().unwrap_or_else(|| controls::icon(None)))
            .accessibility_label(self.entry.text())
            .disabled(!self.entry.state.enabled)
            .selected(self.open)
            .track_focus(&self.focus);
        popover::Popover::new("header-contribution-popover")
            .open(self.open)
            .anchor(Anchor::TopRight)
            .track_focus(&self.focus)
            .p_0()
            .trigger(
                button
                    .on_hover(
                        cx.listener(|view, hover: &bool, window, cx| {
                            view.hover(*hover, window, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            cx.stop_propagation();
                            if view.held {
                                view.close(cx);
                            } else {
                                view.timer = None;
                                view.open = true;
                                view.held = true;
                                cx.notify();
                            }
                        }),
                    ),
            )
            .on_open_change(cx.listener(|view, open: &bool, _, cx| {
                if !open {
                    view.close(cx);
                } else {
                    view.open = true;
                    view.held = true;
                    cx.notify();
                }
            }))
            .content({
                let owner = cx.entity();
                let panel = self.panel.clone();
                move |_, window, _| {
                    div()
                        .id("header-contribution-scroll")
                        .w(width)
                        .max_h(height)
                        .overflow_y_scroll()
                        .child(panel.clone())
                        .on_hover(
                            window.listener_for(&owner, |view, hover: &bool, window, cx| {
                                view.hover(*hover, window, cx)
                            }),
                        )
                }
            })
    }
}
