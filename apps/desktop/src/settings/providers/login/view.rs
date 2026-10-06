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
        let active = !self.closed && !self.cancelling && !self.expired();
        let code = self.state().filter(|_| active).and_then(|state| {
            if let State::Pending { user_code, .. } = state {
                Some(user_code.clone())
            } else {
                None
            }
        });
        let loading = active
            && code.is_none()
            && (self.pending || matches!(self.state(), Some(State::Starting)));
        let selector = if self.expired() {
            "provider-login-code-expired"
        } else if code.is_some() {
            "provider-login-code"
        } else {
            "provider-login-code-placeholder"
        };
        let card = Button::new(selector)
            .debug_selector(move || selector.into())
            .accessibility_label(tr(if code.is_some() {
                "provider_login_copy"
            } else {
                "provider_login_code_placeholder"
            }))
            .custom(ButtonCustomVariant::new(cx).hover(cx.theme().secondary_hover))
            .disabled(code.is_none())
            .w_full()
            .h(rems(6.))
            .bg(cx.theme().group_box)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .when(self.expired(), |card| {
                card.label(tr("provider_login_code_expired"))
            })
            .when(loading, |card| {
                card.child(
                    h_flex()
                        .debug_selector(|| "provider-login-loading".into())
                        .gap_2()
                        .child(spinner::Spinner::new().small())
                        .child(tr("provider_login_starting")),
                )
            })
            .when(!self.expired() && !loading && code.is_none(), |card| {
                card.label(tr("provider_login_code_placeholder"))
            })
            .when_some(code, |card, code| {
                card.child(
                    div()
                        .debug_selector(|| "provider-login-code-text".into())
                        .text_2xl()
                        .font_semibold()
                        .child(code.clone()),
                )
                .on_click(move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                    crate::feedback::toast(
                        window,
                        tr("content_copied"),
                        notification::Notification::success(tr("content_copied")),
                        cx,
                    );
                })
            });
        v_flex()
            .id("provider-login")
            .debug_selector(|| "provider-login".into())
            .w_full()
            .gap_4()
            .child(
                div()
                    .debug_selector(|| "provider-login-code-card".into())
                    .w_full()
                    .child(card),
            )
    }
}
