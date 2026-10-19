//! Provider authorization controls keep presentation separate from admission and recovery.
use super::*;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};

impl Login {
    pub(in crate::settings::providers) fn footer(
        &self,
        owner: &Entity<Self>,
        _: &App,
    ) -> AnyElement {
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
        let manual = self.accepts_callback();
        let footer = dialog::DialogFooter::new()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                Button::new("provider-login-cancel")
                    .debug_selector(|| "provider-login-cancel".into())
                    .loading(self.cancelling && self.pending)
                    .disabled(self.cancelling && self.pending)
                    .label(tr("provider_login_cancel"))
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(dialog::Cancel), cx);
                    }),
            )
            .when_some(
                self.state()
                    .filter(|_| !self.closed && !self.cancelling && self.error.is_none()),
                |row, state| {
                    let url = match state {
                        State::Pending {
                            verification_url, ..
                        } => Some(verification_url),
                        State::Browser {
                            authorization_url, ..
                        } => Some(authorization_url),
                        _ => None,
                    };
                    if let Some(url) = url {
                        let url = url.clone();
                        row.child(
                            Button::new("provider-login-open")
                                .when(!manual, |button| button.primary())
                                .debug_selector(|| "provider-login-open".into())
                                .label(tr(if manual {
                                    "provider_login_browser"
                                } else {
                                    "provider_login_authorize"
                                }))
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
                            .primary()
                            .debug_selector(|| "provider-login-retry".into())
                            .disabled(self.pending)
                            .label(tr("provider_login_retry"))
                            .on_click(move |_, window, cx| {
                                retry.update(cx, |login, cx| login.begin(window, cx))
                            }),
                    )
                },
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
        let active = !self.closed && !self.cancelling && !self.expired() && self.error.is_none();
        v_flex()
            .id("provider-login")
            .debug_selector(|| "provider-login".into())
            .w_full()
            .gap_4()
            .when(
                self.provider.authentication == sailry_protocol::Authentication::ChatGpt,
                |body| {
                    body.on_action(cx.listener(|login, _: &dialog::Confirm, window, cx| {
                        cx.stop_propagation();
                        login.authorize(window, cx);
                    }))
                },
            )
            .child(waiting::surface(
                &self.provider.name,
                if self.expired() {
                    Some(tr("provider_login_code_expired"))
                } else if active {
                    Some(tr(
                        if self.pending
                            || matches!(self.state(), Some(State::Starting | State::Exchanging))
                        {
                            "provider_login_starting"
                        } else {
                            "provider_login_waiting"
                        },
                    ))
                } else {
                    None
                },
                active,
                cx,
            ))
            .when(
                active
                    && !self.pending
                    && self.accepts_callback()
                    && matches!(self.state(), Some(State::Browser { .. })),
                |body| {
                    body.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .debug_selector(|| "provider-login-callback".into())
                                    .child(
                                        input::Input::new(&self.manual)
                                            .aria_label(tr("provider_login_callback")),
                                    ),
                            )
                            .child(
                                Button::new("provider-login-continue")
                                    .primary()
                                    .flex_shrink_0()
                                    .debug_selector(|| "provider-login-continue".into())
                                    .label(tr("provider_login_continue"))
                                    .on_click(
                                        cx.listener(|login, _, window, cx| {
                                            login.submit(window, cx)
                                        }),
                                    ),
                            ),
                    )
                },
            )
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
            .into_any_element()
    }
}

impl Login {
    fn authorize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending
            || self.cancelling
            || self.closed
            || self.error == Some("provider_login_changed")
        {
            return;
        }
        if self.error.is_some() {
            self.begin(window, cx);
        } else if let Some(State::Browser {
            authorization_url, ..
        }) = self.state()
        {
            cx.open_url(authorization_url);
        }
    }
}
