use super::*;

impl Render for Login {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let busy = self.pending || self.active() || self.request.is_some();
        v_flex()
            .id("mcp-authorization")
            .gap_3()
            .child(if self.provider.is_some() {
                self.package.name.clone()
            } else {
                format!("{} / {}", self.package.name, self.server)
            })
            .when(self.provider.is_some(), |body| {
                body.child(
                    Input::new(&self.client_id)
                        .aria_label(tr("plugin_oauth_client_id"))
                        .disabled(busy),
                )
            })
            .when(
                self.provider.is_some_and(|provider| {
                    provider != sailry_protocol::plugin::oauth::Provider::Microsoft
                }),
                |body| {
                    body.child(
                        Input::new(&self.client_secret)
                            .aria_label(tr("plugin_oauth_client_secret"))
                            .disabled(busy),
                    )
                },
            )
            .when(
                self.provider == Some(sailry_protocol::plugin::oauth::Provider::Microsoft),
                |body| {
                    body.child(
                        Input::new(&self.tenant)
                            .aria_label(tr("plugin_oauth_tenant"))
                            .disabled(busy),
                    )
                },
            )
            .when(self.closing || self.active(), |body| {
                body.child(tr(if self.closing {
                    "provider_login_cancelling"
                } else if matches!(self.state(), Some(State::Pending { .. })) {
                    "provider_login_waiting"
                } else {
                    "provider_login_starting"
                }))
            })
    }
}

pub(super) fn footer(login: &Entity<Login>, cx: &App) -> AnyElement {
    let state = login.read(cx);
    let primary = login.clone();
    let cancel = login.clone();
    let url = match state.state() {
        Some(State::Pending { url, .. }) if !state.closing => Some(url.clone()),
        _ => None,
    };
    let waiting = state.active() || state.closing;
    let mut actions = gpui_kit::component::dialog::DialogFooter::new()
        .w_full()
        .gap_2()
        .flex_wrap();
    if state.configured == Some(true) && !waiting && state.request.is_none() {
        let revoke = login.clone();
        actions = actions.child(
            Button::new("mcp-oauth-revoke")
                .debug_selector(|| "mcp-oauth-revoke".into())
                .label(tr("mcp_oauth_remove"))
                .disabled(state.pending)
                .on_click(move |_, window, cx| {
                    let owner = revoke.clone();
                    crate::prompts::confirm(
                        &tr("mcp_oauth_remove"),
                        &tr("mcp_oauth_remove_effect"),
                        tr("mcp_oauth_remove"),
                        window,
                        cx,
                        move |window, cx| {
                            owner.update(cx, |login, cx| {
                                login.prepare(if login.provider.is_some() {
                                    Command::RevokePluginAuthorization {
                                        package: login.package.clone(),
                                    }
                                } else {
                                    Command::RevokeMcpAuthorization {
                                        package: login.package.clone(),
                                        server: login.server.clone(),
                                    }
                                });
                                login.send(window, cx);
                            })
                        },
                    );
                }),
        );
    }
    actions
        .child(
            Button::new("mcp-oauth-cancel")
                .debug_selector(|| "mcp-oauth-cancel".into())
                .label(tr("settings_cancel"))
                .on_click(move |_, window, cx| {
                    cancel.update(cx, |login, cx| {
                        if login.cancel(window, cx) {
                            window.close_dialog(cx);
                        }
                    })
                }),
        )
        .child(
            Button::new("mcp-oauth-submit")
                .debug_selector(|| "mcp-oauth-submit".into())
                .primary()
                .loading(state.pending || (waiting && url.is_none()))
                .disabled(
                    state.pending
                        || (waiting && url.is_none())
                        || (state.request.is_none()
                            && url.is_none()
                            && (state.configured.is_none()
                                || (state.provider.is_none() && !state.can_authorize))),
                )
                .label(tr(if state.pending {
                    "provider_login_starting"
                } else if state.request.is_some() {
                    "plugins_retry"
                } else if url.is_some() {
                    "provider_oauth_open"
                } else {
                    "provider_connect"
                }))
                .on_click(move |_, window, cx| {
                    if let Some(url) = &url {
                        cx.open_url(url);
                        return;
                    }
                    primary.update(cx, |login, cx| {
                        if login.request.is_some() {
                            login.send(window, cx);
                        } else {
                            login.begin(window, cx);
                        }
                    });
                }),
        )
        .into_any_element()
}
