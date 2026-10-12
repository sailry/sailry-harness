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
        let footer = dialog::DialogFooter::new()
            .w_full()
            .justify_end()
            .gap_2()
            .when_some(
                self.state().filter(|_| !self.closed && !self.cancelling),
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
                    .on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(dialog::Cancel), cx);
                    }),
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
        if self.provider.authentication == sailry_protocol::Authentication::ChatGpt {
            return self.browser_body(cx);
        }
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
            .when(
                active
                    && (self.pending
                        || matches!(self.state(), Some(State::Starting | State::Exchanging))),
                |body| {
                    body.child(
                        h_flex()
                            .debug_selector(|| "provider-login-loading".into())
                            .w_full()
                            .gap_2()
                            .py_2()
                            .child(spinner::Spinner::new().small())
                            .child(tr("provider_login_starting")),
                    )
                },
            )
            .when(
                active && !self.pending && matches!(self.state(), Some(State::Browser { .. })),
                |body| {
                    body.child(
                        h_flex()
                            .debug_selector(|| "provider-login-waiting".into())
                            .justify_center()
                            .gap_2()
                            .py_2()
                            .child(tr("provider_login_waiting")),
                    )
                },
            )
            .when(
                active
                    && !self.pending
                    && self.provider.authentication == sailry_protocol::Authentication::ClaudeOAuth
                    && matches!(self.state(), Some(State::Browser { .. })),
                |body| {
                    body.child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .child(
                                div()
                                    .w_full()
                                    .debug_selector(|| "provider-login-callback".into())
                                    .child(
                                        input::Input::new(&self.manual)
                                            .aria_label(tr("provider_login_callback")),
                                    ),
                            )
                            .child(
                                Button::new("provider-login-continue")
                                    .primary()
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
    fn browser_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let loading = self.pending
            || self.cancelling
            || matches!(self.state(), Some(State::Starting | State::Exchanging));
        let retry = self.error.is_some() && self.error != Some("provider_login_changed");
        let url = match self.state() {
            Some(State::Browser {
                authorization_url, ..
            }) => Some(authorization_url.clone()),
            _ => None,
        };
        let interactive = !loading
            && !self.closed
            && self.error != Some("provider_login_changed")
            && (retry || url.is_some());
        let body = v_flex()
            .id("provider-login")
            .debug_selector(|| "provider-login".into())
            .w_full()
            .items_center()
            .text_center()
            .py_6()
            .gap_4()
            .child(
                img("branding/sailry-mark.svg")
                    .debug_selector(|| "provider-login-logo".into())
                    .size_16(),
            )
            .child(if loading {
                h_flex()
                    .debug_selector(|| "provider-login-loading".into())
                    .justify_center()
                    .gap_2()
                    .py_2()
                    .child(spinner::Spinner::new().small())
                    .child(tr("provider_login_starting"))
                    .into_any_element()
            } else {
                h_flex()
                    .when(!retry, |row| {
                        row.debug_selector(|| "provider-login-waiting".into())
                    })
                    .justify_center()
                    .py_2()
                    .child(tr(if retry {
                        "provider_login_retry"
                    } else {
                        "provider_login_waiting"
                    }))
                    .into_any_element()
            });
        if !interactive {
            return body.into_any_element();
        }
        let id = if retry {
            "provider-login-retry"
        } else {
            "provider-login-open"
        };
        Button::new(id)
            .debug_selector(move || id.into())
            .ghost()
            .w_full()
            .h_auto()
            .p_0()
            .when(!retry, |button| button.cursor_pointer())
            .accessibility_label(tr(if retry {
                "provider_login_retry"
            } else {
                "provider_login_authorize"
            }))
            .child(body)
            // Dialog binds Enter to Confirm before the native button's key-up.
            .on_action(cx.listener(|login, _: &dialog::Confirm, window, cx| {
                cx.stop_propagation();
                login.authorize(window, cx);
            }))
            .on_click(cx.listener(|login, _, window, cx| {
                login.authorize(window, cx);
            }))
            .into_any_element()
    }

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
