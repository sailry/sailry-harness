use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{dialog::DialogFooter, spinner::Spinner};

impl EventEmitter<DismissEvent> for Connections {}

impl Connections {
    pub(super) fn open_dialog(
        &mut self,
        sharing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel(cx);
        self.sharing = sharing;
        self.pin.update(cx, |pin, cx| pin.set_value("", window, cx));
        let owner = cx.entity().downgrade();
        let closing = owner.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let Some(owner) = owner.upgrade() else {
                return dialog;
            };
            dialog
                .map(|dialog| {
                    if sharing {
                        dialog.p_6().title(
                            div()
                                .w_full()
                                .text_center()
                                .text_xl()
                                .font_semibold()
                                .child(tr("pairing_local_code")),
                        )
                    } else {
                        dialog.form_title(tr("connections_pair"))
                    }
                })
                .w(px(400.).min(window.viewport_size().width - px(48.)))
                .overlay_closable(false)
                .on_close({
                    let closing = closing.clone();
                    move |_, _, cx| {
                        let _ = closing.update(cx, |this, cx| {
                            this.cancel(cx);
                            this.sharing = false;
                            this.dialog = None;
                            cx.notify();
                        });
                    }
                })
                .child(owner.update(cx, |this, cx| {
                    let mut body = v_flex().items_center().gap_3().py_5();
                    if sharing {
                        body = this.shared_code(window, cx);
                    } else {
                        body = body.child(
                            OtpInput::new(&this.pin)
                                .groups(2)
                                .large()
                                .disabled(this.stop.is_some()),
                        );
                    }
                    body
                }))
                .when(!sharing, |dialog| {
                    dialog.footer(owner.update(cx, |this, cx| {
                        DialogFooter::new()
                            .child(
                                Button::new("pairing-cancel")
                                    .debug_selector(|| "pairing-cancel".into())
                                    .label(tr("settings_cancel"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel(cx);
                                        this.dialog = None;
                                        window.close_dialog(cx);
                                    })),
                            )
                            .when(!sharing, |footer| {
                                footer.child(
                                    Button::new("pairing-connect")
                                        .debug_selector(|| "pairing-connect".into())
                                        .primary()
                                        .label(tr("pairing_connect"))
                                        .loading(this.stop.is_some())
                                        .disabled(
                                            this.stop.is_some()
                                                || this.pin.read(cx).value().len() != 6,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| this.pair(cx))),
                                )
                            })
                    }))
                })
        });
        self.dialog = window.focused(cx);
        if sharing {
            self.share(cx);
        }
    }
    fn shared_code(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let width = ((window.viewport_size().width - px(96.) - px(40.)) / 6.).min(px(48.));
        let body = v_flex()
            .w_full()
            .items_center()
            .gap_4()
            .pb_2()
            .child(self.relay_fields(cx));
        let code =
            if let Some(code) = &self.code {
                if self.display.read(cx).value().as_str() != code {
                    self.display.update(cx, |display, cx| {
                        display.set_value(code.clone(), window, cx)
                    });
                }
                // Kit b79f4ce's styled OTP has no read-only palette or rectangular cells.
                // Reuse its OTP primitive and state, with theme-based presentation for sharing.
                gpui_kit::base::OtpInput::new(&self.display)
                    .disabled(true)
                    .child(h_flex().gap_2().children(code.chars().enumerate().map(
                        |(index, digit)| {
                            div()
                                .debug_selector(move || format!("pairing-digit-{index}"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(width)
                                .h_16()
                                .rounded(cx.theme().radius_lg)
                                .bg(cx.theme().secondary)
                                .text_color(cx.theme().success)
                                .text_3xl()
                                .font_semibold()
                                .child(digit.to_string())
                        },
                    )))
                    .into_any_element()
            } else if self.stop.is_some() {
                h_flex()
                    .h_16()
                    .gap_2()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new())
                    .child(tr(if self.checking {
                        "pairing_relay_checking"
                    } else {
                        "pairing_preparing"
                    }))
                    .into_any_element()
            } else if self.relay_feedback.is_some() {
                Button::new("pairing-relay-retry")
                    .debug_selector(|| "pairing-relay-retry".into())
                    .label(tr("settings_retry"))
                    .on_click(cx.listener(|this, _, _, cx| this.share(cx)))
                    .into_any_element()
            } else {
                div().h_16().into_any_element()
            };
        body.child(div().py_2().child(code))
            .when(self.code.is_some(), |body| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("pairing_refresh_hint")),
                )
            })
            .child(
                div()
                    .text_sm()
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("pairing_share_hint")),
            )
    }
}
