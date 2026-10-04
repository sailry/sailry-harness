use super::*;

impl Render for Login {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.pending || self.active() || self.request.is_some();
        let mut body = v_flex()
            .id("mcp-authorization")
            .gap_3()
            .child(format!("{} / {}", self.package.name, self.server))
            .child(
                Input::new(&self.client_id)
                    .aria_label(tr("mcp_oauth_client_id"))
                    .disabled(busy),
            )
            .child(tr(if self.closing {
                "provider_login_cancelling"
            } else {
                match self.state() {
                    Some(State::Starting | State::Exchanging) => "provider_login_starting",
                    Some(State::Pending { .. }) => "provider_login_waiting",
                    Some(State::Connected(_)) => "provider_login_connected",
                    Some(State::Cancelled) => "provider_login_cancelled",
                    _ if self.configured == Some(true) => "provider_login_connected",
                    _ => "mcp_oauth_not_connected",
                }
            }));
        if let Some(State::Pending { url, .. }) = self.state() {
            let url = url.clone();
            body = body.child(
                Button::new("mcp-oauth-browser")
                    .debug_selector(|| "mcp-oauth-browser".into())
                    .primary()
                    .label(tr("provider_oauth_open"))
                    .disabled(self.closing)
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }
        let mut actions = gpui_kit::component::dialog::DialogFooter::new()
            .w_full()
            .mt_3()
            .flex_wrap();
        if self.request.is_some() {
            actions = actions.child(
                Button::new("mcp-oauth-retry")
                    .debug_selector(|| "mcp-oauth-retry".into())
                    .label(tr("plugins_retry"))
                    .disabled(self.pending)
                    .on_click(cx.listener(|login, _, window, cx| login.send(window, cx))),
            );
        } else if !self.active() {
            actions = actions.child(
                Button::new("mcp-oauth-connect")
                    .debug_selector(|| "mcp-oauth-connect".into())
                    .primary()
                    .label(tr("provider_connect"))
                    .disabled(self.configured.is_none())
                    .on_click(cx.listener(|login, _, window, cx| login.begin(window, cx))),
            );
            if self.configured == Some(true) {
                actions = actions.child(
                    Button::new("mcp-oauth-revoke")
                        .debug_selector(|| "mcp-oauth-revoke".into())
                        .label(tr("mcp_oauth_remove"))
                        .on_click(cx.listener(|_, _, window, cx| {
                            let owner = cx.entity();
                            crate::prompts::confirm(
                                &tr("mcp_oauth_remove"),
                                &tr("mcp_oauth_remove_effect"),
                                tr("mcp_oauth_remove"),
                                window,
                                cx,
                                move |window, cx| {
                                    owner.update(cx, |login, cx| {
                                        login.prepare(Command::RevokeMcpAuthorization {
                                            package: login.package.clone(),
                                            server: login.server.clone(),
                                        });
                                        login.send(window, cx);
                                    })
                                },
                            );
                        })),
                );
            }
        }
        actions = actions.child(
            Button::new("mcp-oauth-cancel")
                .debug_selector(|| "mcp-oauth-cancel".into())
                .label(tr("settings_cancel"))
                .on_click(cx.listener(|login, _, window, cx| {
                    if login.cancel(window, cx) {
                        window.close_dialog(cx);
                    }
                })),
        );
        body.child(actions)
    }
}
