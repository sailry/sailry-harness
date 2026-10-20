//! Provider authorization maps its admission and recovery state onto the shared dialog.
use super::*;
use crate::ui::authorization::{Authorization, Flow, TILE};
use gpui_kit::component::button::{Button, ButtonVariants};

impl Login {
    fn flow(&self) -> Flow {
        let prompt = |state: Option<&State>| match state {
            Some(State::Pending {
                verification_url,
                user_code,
                ..
            }) => (
                Some(SharedString::from(user_code.clone())),
                Some(SharedString::from(verification_url.clone())),
            ),
            Some(State::Browser {
                authorization_url, ..
            }) => (None, Some(SharedString::from(authorization_url.clone()))),
            _ => (None, None),
        };
        if self.expired() {
            let (code, url) = prompt(self.prompt.as_ref());
            return Flow::Expired { code, url };
        }
        if self.error.is_some() {
            let (code, url) = prompt(self.prompt.as_ref());
            return Flow::Failed { code, url };
        }
        if self.pending && !self.cancelling {
            return Flow::Starting;
        }
        match self
            .state()
            .or(self.prompt.as_ref().filter(|_| self.cancelling))
        {
            Some(State::Pending {
                verification_url,
                user_code,
                expires_at_ms,
            }) => Flow::Device {
                code: user_code.clone().into(),
                url: verification_url.clone().into(),
                expires_at_ms: Some(*expires_at_ms),
            },
            Some(State::Browser {
                authorization_url, ..
            }) => Flow::Browser {
                url: authorization_url.clone().into(),
            },
            _ => Flow::Starting,
        }
    }

    fn callback_input(&self, cx: &mut Context<Self>) -> AnyElement {
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
                        input::Input::new(&self.manual).aria_label(tr("provider_login_callback")),
                    ),
            )
            .child(
                Button::new("provider-login-continue")
                    .primary()
                    .flex_shrink_0()
                    .debug_selector(|| "provider-login-continue".into())
                    .label(tr("provider_login_continue"))
                    .on_click(cx.listener(|login, _, window, cx| login.submit(window, cx))),
            )
            .into_any_element()
    }
}

impl Render for Login {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = !self.closed && !self.cancelling && !self.expired() && self.error.is_none();
        let mut dialog = Authorization::new(
            super::super::list::tile(&self.channel, TILE),
            self.provider.name.clone(),
            self.flow(),
        )
        .cancelling(self.cancelling && self.pending)
        .busy(self.pending);
        if active
            && !self.pending
            && self.accepts_callback()
            && matches!(self.state(), Some(State::Browser { .. }))
        {
            dialog = dialog.extra(self.callback_input(cx));
        }
        if !self.cancelling && self.error != Some("provider_login_changed") {
            let owner = cx.entity();
            dialog = dialog.on_refresh(move |window, cx| {
                owner.update(cx, |login, cx| login.begin(window, cx))
            });
        }
        div()
            .id("provider-login")
            .debug_selector(|| "provider-login".into())
            .w_full()
            .when(
                self.provider.authentication == sailry_protocol::Authentication::ChatGpt,
                |body| {
                    body.on_action(cx.listener(|login, _: &dialog::Confirm, window, cx| {
                        cx.stop_propagation();
                        login.authorize(window, cx);
                    }))
                },
            )
            .child(dialog)
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
