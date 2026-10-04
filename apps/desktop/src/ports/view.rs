use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    form::Field,
    input::Input,
    scroll::ScrollableElement,
};
use sailry_client::ports::State;

impl Workspace {
    pub(crate) fn show(owner: Entity<Self>, window: &mut Window, cx: &mut App) {
        if window.has_active_dialog(cx) {
            return;
        }
        owner.update(cx, |owner, cx| {
            owner.error = None;
            cx.notify();
        });
        window.open_dialog(cx, move |dialog, window, cx| {
            let submitting = owner.clone();
            let cancelling = owner.clone();
            let busy = owner.read(cx).busy;
            dialog
                .form_title(tr("port_add"))
                .w((window.viewport_size().width - px(48.)).min(px(480.)))
                .close_button(!busy)
                .overlay_closable(!busy)
                .keyboard(!busy)
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("port-cancel")
                                .debug_selector(|| "port-cancel".into())
                                .label(tr("settings_cancel"))
                                .disabled(busy)
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            Button::new("port-create")
                                .debug_selector(|| "port-create".into())
                                .primary()
                                .label(tr("port_create"))
                                .loading(busy)
                                .disabled(busy)
                                .on_click(|_, window, cx| {
                                    window
                                        .dispatch_action(Box::new(Confirm { secondary: false }), cx)
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submitting.update(cx, |owner, cx| owner.open(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| !cancelling.read(cx).busy)
                .child(owner.clone())
        });
    }

    pub(crate) fn header(&self, owner: Entity<Self>, cx: &App) -> AnyElement {
        let button = Button::new("host-ports");
        h_flex()
            .max_w(px(320.))
            .gap_2()
            .flex_wrap()
            .justify_end()
            .children(
                self.entries
                    .iter()
                    .enumerate()
                    .take(3)
                    .map(|(index, entry)| {
                        let port = entry.forwarder.local_port;
                        let remote = entry.forwarder.remote_port;
                        let url = entry.url.clone();
                        Button::new(("host-port", index))
                            .small()
                            .ghost()
                            .icon(IconName::Globe)
                            .label(remote.to_string())
                            .debug_selector(move || format!("host-port-{index}"))
                            .tooltip(format!("{remote} → 127.0.0.1:{port}"))
                            .disabled(*entry.forwarder.state.borrow() != State::Listening)
                            .on_click(move |_, _, cx| cx.open_url(&url))
                    }),
            )
            .child(
                button
                    .small()
                    .ghost()
                    .icon(IconName::Network)
                    .label(tr("port_title"))
                    .debug_selector(|| "host-ports".into())
                    .on_click(move |_, window, cx| {
                        picker::open(owner.clone(), window, cx);
                    }),
            )
            .text_color(cx.theme().foreground)
            .into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.busy;
        v_flex()
            .id("port-content")
            .max_h((window.viewport_size().height - px(240.)).max(px(120.)))
            .overflow_y_scrollbar()
            .debug_selector(|| "live-ports".into())
            .child(
                gpui_kit::component::form::Form::vertical()
                    .gap_4()
                    .child(
                        Field::new().label(tr("port_remote")).child(
                            Input::new(&self.port)
                                .readonly(locked)
                                .aria_label(tr("port_remote")),
                        ),
                    )
                    .child(
                        Field::new().label(tr("port_local")).child(
                            Input::new(&self.local)
                                .readonly(locked)
                                .aria_label(tr("port_local")),
                        ),
                    ),
            )
    }
}
