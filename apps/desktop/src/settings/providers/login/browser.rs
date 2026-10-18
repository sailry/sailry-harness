//! The controller receives a loopback callback; the Node owns token exchange.
use super::*;

impl Login {
    pub(super) fn receive(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.browser_reply = Some(sender);
        let job = self.binding.runtime.spawn(callback.receive(
            url,
            crate::oauth::page::response(receiver),
            self.callback_stop.clone(),
        ));
        self.receiver = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |login, window, cx| {
                if login.closed
                    || login.cancelling
                    || login.completion.is_some()
                    || !login.state().is_some_and(State::active)
                {
                    return;
                }
                match result {
                    Ok(Ok(callback)) => {
                        let attempt = login.attempt.as_ref().expect("observed login attempt").id;
                        login.completion = Some(
                            login
                                .binding
                                .client
                                .prepare(Command::CompleteProviderLogin { attempt, callback }),
                        );
                        login.complete(window, cx);
                    }
                    _ => {
                        crate::feedback::error("", &tr("provider_login_failed"), window, cx);
                        // A consumed listener cannot resume this browser attempt.
                        login.cancel(window, cx);
                    }
                }
            });
        }));
    }

    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending
            || self.closed
            || self.cancelling
            || !self.accepts_callback()
            || !matches!(self.state(), Some(State::Browser { .. }))
        {
            return;
        }
        let value = self.manual.read(cx).value().trim().to_owned();
        if value.is_empty() {
            crate::feedback::error("", &tr("provider_login_callback_required"), window, cx);
            return;
        }
        let Some(attempt) = self.attempt.as_ref() else {
            return;
        };
        self.completion = Some(self.binding.client.prepare(Command::CompleteProviderLogin {
            attempt: attempt.id,
            callback: sailry_protocol::Secret::new(value),
        }));
        self.callback_stop.cancel();
        self.callback = None;
        self.receiver = None;
        self.complete(window, cx);
    }

    pub(super) fn complete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed || self.cancelling {
            return;
        }
        let Some(request) = self.completion.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        // Retain admission ownership when the editor closes during exchange.
        let keep_alive = cx.entity();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let _keep_alive = keep_alive;
            let result = job.await;
            let _ = owner.update_in(cx, |login, window, cx| {
                login.pending = false;
                if login.closed && !login.cancelling {
                    return;
                }
                match result {
                    Ok(Ok(Output::ProviderLogin(attempt)))
                        if login.attempt.as_ref() == Some(&attempt) => {}
                    Ok(Err(error)) => login.error = Some(error_key(&error)),
                    _ => login.error = Some("provider_login_unknown"),
                }
                if login.cancelling {
                    login.cancel(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
