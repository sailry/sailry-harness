//! The native device-login dialog keeps presentation separate from admission and recovery.
use super::*;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};

impl Login {
    pub(super) fn footer(&self, owner: &Entity<Self>, _: &App) -> AnyElement {
        if self.expired() {
            let refresh = owner.clone();
            return div()
                .debug_selector(|| "provider-login-footer".into())
                .w_full()
                .child(
                    dialog::DialogFooter::new().w_full().child(
                        Button::new("provider-login-refresh")
                            .primary()
                            .debug_selector(|| "provider-login-refresh".into())
                            .label(tr("provider_login_refresh"))
                            .disabled(self.pending)
                            .on_click(move |_, window, cx| {
                                refresh.update(cx, |login, cx| login.begin(window, cx))
                            }),
                    ),
                )
                .into_any_element();
        }
        let retry = owner.clone();
        let footer = dialog::DialogFooter::new()
            .w_full()
            .justify_end()
            .gap_2()
            .when_some(
                self.state().filter(|_| !self.closed && !self.cancelling),
                |row, state| {
                    if let State::Pending {
                        verification_url, ..
                    } = state
                    {
                        let url = verification_url.clone();
                        row.child(
                            Button::new("provider-login-open")
                                .primary()
                                .debug_selector(|| "provider-login-open".into())
                                .label(tr("provider_login_authorize"))
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    } else {
                        row
                    }
                },
            )
            .when(
                self.error.is_some()
                    && !self.cancelling
                    && self.error != Some("provider_login_changed"),
                |row| {
                    row.child(
                        Button::new("provider-login-retry")
                            .debug_selector(|| "provider-login-retry".into())
                            .disabled(self.pending)
                            .label(tr("provider_login_retry"))
                            .on_click(move |_, window, cx| {
                                retry.update(cx, |login, cx| login.begin(window, cx))
                            }),
                    )
                },
            )
            .child(
                Button::new("provider-login-cancel")
                    .debug_selector(|| "provider-login-cancel".into())
                    .loading(self.cancelling && self.pending)
                    .disabled(self.cancelling && self.pending)
                    .label(tr("provider_login_cancel"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(dialog::Cancel), cx)),
            );
        div()
            .debug_selector(|| "provider-login-footer".into())
            .w_full()
            .child(footer)
            .into_any_element()
    }
}

impl Render for Login {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = !self.closed && !self.cancelling && !self.expired();
        v_flex()
            .id("provider-login")
            .debug_selector(|| "provider-login".into())
            .w_full()
            .gap_4()
            .when(self.expired(), |body| {
                body.child(
                    Button::new("provider-login-code-expired")
                        .debug_selector(|| "provider-login-code-expired".into())
                        .custom(ButtonCustomVariant::new(cx))
                        .disabled(true)
                        .w_full()
                        .h(rems(6.))
                        .bg(cx.theme().group_box)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg)
                        .label(tr("provider_login_code_expired")),
                )
            })
            .when(active && self.pending && self.state().is_none(), |body| {
                body.child(
                    h_flex()
                        .w_full()
                        .h(rems(6.))
                        .justify_center()
                        .child(spinner::Spinner::new()),
                )
            })
            .when_some(self.state().filter(|_| active), |body, state| {
                if let State::Pending { user_code, .. } = state {
                    let code = user_code.clone();
                    body.child(
                        Button::new("provider-login-code")
                            .debug_selector(|| "provider-login-code".into())
                            .accessibility_label(tr("provider_login_copy"))
                            .custom(ButtonCustomVariant::new(cx).hover(cx.theme().secondary_hover))
                            .w_full()
                            .h(rems(6.))
                            .bg(cx.theme().group_box)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius_lg)
                            .child(
                                div()
                                    .debug_selector(|| "provider-login-code-text".into())
                                    .text_2xl()
                                    .font_semibold()
                                    .child(user_code.clone()),
                            )
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                                crate::feedback::toast(
                                    window,
                                    tr("content_copied"),
                                    notification::Notification::success(tr("content_copied")),
                                    cx,
                                );
                            }),
                    )
                } else {
                    body
                }
            })
    }
}
