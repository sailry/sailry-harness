//! Kit Accordion fixes its trailing chevron, so this header composes Kit controls.
//! The shared selection still owns expansion; the one trailing button always stops.
use super::*;

#[derive(IntoElement)]
pub(super) struct Header {
    pub id: RequestId,
    pub panel: WeakEntity<Panel>,
    pub open: bool,
    pub disabled: bool,
    pub status: AnyElement,
    pub command: SharedString,
}

struct State {
    hover: bool,
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl RenderOnce for Header {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id;
        let state = window.use_keyed_state(
            SharedString::from(format!("background-header-{id}")),
            cx,
            |window, cx| {
                let focus = cx.focus_handle().tab_stop(false);
                State {
                    hover: false,
                    _subscriptions: [
                        cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
                        cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
                    ],
                    focus,
                }
            },
        );
        let focus = state.read(cx).focus.clone();
        let stopping = state.read(cx).hover || focus.contains_focused(window, cx);
        let stop_panel = self.panel.clone();
        h_flex()
            .id(SharedString::from(format!("background-header-{id}")))
            .debug_selector(move || format!("background-header-{id}"))
            .track_focus(&focus)
            .w_full()
            .h_6()
            .min_w_0()
            .gap_2()
            .on_hover(move |hovered, _, cx| {
                state.update(cx, |state, cx| {
                    state.hover = *hovered;
                    cx.notify();
                })
            })
            .child(self.status)
            .child(
                Button::new(SharedString::from(format!("background-toggle-{id}")))
                    .debug_selector(move || format!("background-toggle-{id}"))
                    .custom(
                        gpui_kit::component::button::ButtonCustomVariant::new(cx)
                            .foreground(cx.theme().muted_foreground)
                            .active(*cx.theme().tokens.button_active),
                    )
                    .small()
                    .flex_1()
                    .min_w_0()
                    .px_0()
                    .justify_start()
                    .text_color(cx.theme().muted_foreground)
                    .accessibility_label(self.command.clone())
                    .child(
                        div()
                            .debug_selector(move || format!("background-command-{id}"))
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .child(self.command),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = self.panel.update(cx, |panel, cx| {
                            let selected = (*panel.selected.borrow() != Some(id)).then_some(id);
                            panel.state.output = None;
                            panel.selected.send_replace(selected);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("background-stop-{id}")))
                    .debug_selector(move || format!("background-stop-{id}"))
                    .ghost()
                    .small()
                    .flex_shrink_0()
                    .w_6()
                    .px_0()
                    .child(
                        div()
                            .debug_selector(move || {
                                format!(
                                    "background-{}-{id}",
                                    if stopping { "stop-icon" } else { "chevron" }
                                )
                            })
                            .child(
                                Icon::new(if stopping {
                                    IconName::Square
                                } else if self.open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .small(),
                            ),
                    )
                    .accessibility_label(tr("background_end"))
                    .tooltip(tr("background_end"))
                    .disabled(self.disabled)
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let _ =
                            stop_panel.update(cx, |panel, cx| panel.stop_command(id, window, cx));
                    }),
            )
    }
}
