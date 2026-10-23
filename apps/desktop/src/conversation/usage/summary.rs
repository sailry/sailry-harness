use super::*;
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use std::time::Duration;

pub(in crate::conversation) fn render(groups: Vec<Group>, _cx: &App) -> AnyElement {
    let value = groups
        .iter()
        .flat_map(|group| &group.metrics)
        .next()
        .map(|metric| metric.value.clone())
        .unwrap_or_else(|| tr("composer_metric_unknown"));
    Compact { groups, value }.into_any_element()
}

#[derive(IntoElement)]
struct Compact {
    groups: Vec<Group>,
    value: SharedString,
}

#[derive(Default)]
struct Visibility {
    open: bool,
    held: bool,
    trigger: bool,
    content: bool,
    revision: u64,
}

impl Visibility {
    fn hover(&mut self, trigger: bool, hovered: bool) -> Option<bool> {
        self.revision = self.revision.wrapping_add(1);
        if trigger {
            self.trigger = hovered;
        } else {
            self.content = hovered;
        }
        self.pending()
    }

    fn pending(&self) -> Option<bool> {
        (!self.held).then_some(self.trigger || self.content)
    }

    fn hold(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.open = true;
        self.held = true;
    }

    fn dismiss(&mut self) {
        *self = Self {
            revision: self.revision.wrapping_add(1),
            ..Self::default()
        };
    }

    fn settle(&mut self, revision: u64, open: bool) -> bool {
        if self.revision != revision || self.pending() != Some(open) {
            return false;
        }
        self.open = open;
        true
    }
}

struct State {
    visibility: Visibility,
    focus: FocusHandle,
    timer: Option<Task<()>>,
}

impl State {
    fn hover(&mut self, trigger: bool, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.timer = None;
        let Some(open) = self.visibility.hover(trigger, hovered) else {
            return;
        };
        if open == self.visibility.open {
            return;
        }
        let delay = Duration::from_millis(if open { 600 } else { 300 });
        let revision = self.visibility.revision;
        self.timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update_in(cx, |state, _, cx| {
                if state.visibility.settle(revision, open) {
                    cx.notify();
                }
            });
        }));
    }

    fn changed(&mut self, open: bool, cx: &mut Context<Self>) {
        self.timer = None;
        if open {
            self.visibility.hold();
        } else {
            self.visibility.dismiss();
        }
        cx.notify();
    }
}

impl RenderOnce for Compact {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Kit HoverCard does not expose desktop click or keyboard activation.
        // Match the existing header adapter: Kit Popover owns focus/dismissal,
        // while this local state joins delayed hover with click-to-hold.
        let state = window.use_keyed_state("composer-statistics-hover", cx, |_, cx| State {
            visibility: Visibility::default(),
            focus: cx.focus_handle(),
            timer: None,
        });
        let open = state.read(cx).visibility.open;
        let held = state.read(cx).visibility.held;
        let focus = state.read(cx).focus.clone();
        let target = (open && !held).then(|| window.focused(cx)).flatten();
        let groups = Rc::new(self.groups);
        crate::theme::popover(popover::Popover::new("composer-stats-popover"), cx)
            .open(open)
            .anchor(Anchor::BottomRight)
            .when_some(target, |popover, target| popover.track_focus(&target))
            .p_0()
            .trigger(
                Button::new("composer-stats")
                    .custom(ButtonCustomVariant::new(cx).foreground(cx.theme().muted_foreground))
                    .small()
                    .rounded_full()
                    .flex_shrink_0()
                    .icon(IconName::ChartPie)
                    .label(self.value)
                    .accessibility_label(tr("composer_statistics"))
                    .track_focus(&focus)
                    .debug_selector(|| "composer-stats".into())
                    .on_hover(
                        window.listener_for(&state, |state, hovered: &bool, window, cx| {
                            state.hover(true, *hovered, window, cx)
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        window.listener_for(&state, |state, _, _, cx| {
                            cx.stop_propagation();
                            state.changed(!state.visibility.held, cx);
                        }),
                    ),
            )
            .on_open_change(
                window.listener_for(&state, |state, open: &bool, _, cx| state.changed(*open, cx)),
            )
            .content(move |popover, window, cx| {
                // A held popup uses Kit's content focus node, which owns Cancel.
                // Hover previews leave the current editing focus in place.
                if held {
                    let focus = popover.focus_handle(cx);
                    if !focus.contains_focused(window, cx) {
                        focus.focus(window, cx);
                    }
                }
                div()
                    .id("composer-statistics-content")
                    .p_3()
                    .on_hover(
                        window.listener_for(&state, |state, hovered: &bool, window, cx| {
                            state.hover(false, *hovered, window, cx)
                        }),
                    )
                    .child(
                        v_flex()
                            .w((window.viewport_size().width - px(56.))
                                .min(px(POPOVER_WIDTH - 2. * POPOVER_INSET.1)))
                            .max_h((window.viewport_size().height - px(80.)).min(px(560.)))
                            .debug_selector(|| "composer-statistics-panel".into())
                            .child(
                                v_flex().flex_1().overflow_hidden().child(
                                    v_flex()
                                        .id("composer-statistics-scroll")
                                        .size_full()
                                        .children(groups.iter().enumerate().map(
                                            |(index, group)| {
                                                v_flex()
                                                    .gap_1p5()
                                                    .when(index > 0, |body| {
                                                        body.child(Separator::horizontal())
                                                    })
                                                    .child((group.summary)(cx))
                                            },
                                        ))
                                        .overflow_y_scrollbar(),
                                ),
                            ),
                    )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn hover_crosses_into_content() {
        let mut visibility = Visibility::default();
        assert_eq!(visibility.hover(true, true), Some(true));
        visibility.open = true;
        assert_eq!(visibility.hover(true, false), Some(false));
        assert_eq!(visibility.hover(false, true), Some(true));
        assert_eq!(visibility.hover(true, false), Some(true));
        assert_eq!(visibility.hover(false, false), Some(false));
    }

    #[test]
    fn click_keeps_the_preview_open() {
        let mut visibility = Visibility::default();
        _ = visibility.hover(true, true);
        visibility.open = true;
        visibility.hold();
        _ = visibility.hover(true, false);
        _ = visibility.hover(false, false);
        assert!(visibility.open);
        assert_eq!(visibility.pending(), None);
    }

    #[test]
    fn dismissal_clears_pending_hover() {
        let mut visibility = Visibility::default();
        _ = visibility.hover(true, true);
        _ = visibility.hover(false, true);
        visibility.hold();
        visibility.dismiss();
        assert!(!visibility.open);
        assert!(!visibility.held);
        assert_eq!(visibility.pending(), Some(false));
    }

    #[test]
    fn reentry_invalidates_an_earlier_timer() {
        let mut visibility = Visibility::default();
        _ = visibility.hover(true, true);
        let previous = visibility.revision;
        _ = visibility.hover(true, false);
        _ = visibility.hover(true, true);
        assert!(!visibility.settle(previous, true));
        assert!(!visibility.open);
        assert!(visibility.settle(visibility.revision, true));
        assert!(visibility.open);
        let closing = visibility.revision;
        visibility.hold();
        assert!(!visibility.settle(closing, false));
        visibility.dismiss();
        assert!(!visibility.settle(closing, true));
    }
}
