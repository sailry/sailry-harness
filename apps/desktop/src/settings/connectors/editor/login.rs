//! Observe one durable attempt and adapt its controller-local loopback callback.
use super::*;
use crate::ui::authorization::{Authorization, Flow, TILE};
use sailry_client::oauth::Callback;

enum Message {
    Attempt(connector::Attempt),
    State(LoginState),
    Saved(Connection),
    Failed(Fault),
}

impl Editor {
    pub(super) fn submit(
        &mut self,
        registration: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.closed || (!registration && !self.required(cx)) {
            return;
        }
        let Some(kind) = self.kind().cloned() else {
            return;
        };
        let Some(auth) = self.method().cloned() else {
            return;
        };
        let mut input = self.values(cx);
        input.registration = registration;
        let interactive =
            matches!(auth, Auth::Oauth2 { .. } | Auth::McpOauth { .. }) || registration;
        let needs_callback = matches!(
            auth,
            Auth::Oauth2 {
                flow: connector::Flow::Pkce,
                ..
            } | Auth::McpOauth { .. }
        ) || registration
            && matches!(&auth, Auth::Application { registration: Some(registration), .. } if matches!(&registration.flow, sailry_protocol::plugin::model_auth::Flow::Script { callback: Some(_), .. }));
        let callback_path = match &auth {
            Auth::Application {
                registration: Some(registration),
                ..
            } => match &registration.flow {
                sailry_protocol::plugin::model_auth::Flow::Script {
                    callback: Some(callback),
                    ..
                } => callback.path.as_str(),
                _ => "/auth/callback",
            },
            _ => "/auth/callback",
        };
        // A replay must retain the redirect captured by the original durable request.
        if self.request.is_none() {
            self.callback = if needs_callback {
                match Callback::bind(callback_path) {
                    Ok(callback) => Some(callback),
                    Err(error) => {
                        failure(error, window, cx);
                        return;
                    }
                }
            } else {
                None
            };
        }
        let callback = match self.callback.as_ref().map(Callback::duplicate).transpose() {
            Ok(callback) => callback,
            Err(error) => {
                failure(error, window, cx);
                return;
            }
        };
        input.redirect = callback.as_ref().map(|callback| callback.redirect.clone());
        let connection = self
            .attempt
            .as_ref()
            .map(|attempt| attempt.connection)
            .or_else(|| self.connection.as_ref().map(|connection| connection.id));
        if let Some(current) = self
            .owner
            .read(cx)
            .connectors
            .rows
            .iter()
            .find(|current| Some(current.id) == connection)
            .cloned()
        {
            self.connection = Some(current);
        }
        let expected_revision = self
            .connection
            .as_ref()
            .filter(|current| Some(current.id) == connection)
            .map(|connection| connection.revision)
            .or_else(|| self.attempt.as_ref().map(|attempt| attempt.revision));
        let editing = self.editing
            && !self.reconnecting
            && matches!(auth, Auth::Oauth2 { .. } | Auth::McpOauth { .. })
            && !registration;
        let command = if editing {
            Command::UpdateConnection {
                connection: connection.expect("edited connection"),
                expected_revision: expected_revision.expect("edited revision"),
                input,
            }
        } else {
            Command::StartConnectorLogin {
                package: kind.package,
                connector: kind.declaration.id,
                connection,
                expected_revision,
                method: auth.kind(),
                input,
            }
        };
        let client = self.binding.client.clone();
        let request = self
            .request
            .clone()
            .unwrap_or_else(|| client.prepare(command));
        self.request = Some(request.clone());
        let cancellation = client.prepare(Command::CancelConnectorLogin {
            attempt: request.id,
        });
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.pending = true;
        self.flow = interactive.then_some(LoginState::Starting);
        let stop = self.stop.clone();
        let observed = stop.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let job = self.binding.runtime.spawn(async move {
            let operation = async {
                let output = client.execute(request).await?;
                let attempt = match output {
                    Output::ConnectorLogin(attempt) => attempt,
                    Output::Connection(connection) => {
                        let _ = sender.send(Message::Saved(connection));
                        return Ok(());
                    }
                    _ => return Err(Fault::invalid("connector login expected")),
                };
                let _ = sender.send(Message::Attempt(attempt.clone()));
                let mut subscription = client.subscribe_connector_login(attempt.id).await?;
                let mut callback = callback;
                let mut callback_task = None;
                let mut browser_reply: Option<tokio::sync::oneshot::Sender<crate::oauth::page::Outcome>> = None;
                loop {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => {
                            if let Some(reply) = browser_reply.take() {
                                let _: Result<(), crate::oauth::page::Outcome> = reply.send(crate::oauth::page::Outcome::Cancelled);
                            }
                            client.execute(cancellation.clone()).await?;
                            return Ok(());
                        },
                        returned = async {
                            match &mut callback_task {
                                Some(task) => task.await,
                                None => std::future::pending().await,
                            }
                        } => {
                            callback_task = None;
                            client.execute(client.prepare(Command::SubmitConnectorInput {
                                attempt: attempt.id,
                                input: returned?,
                            })).await?;
                        },
                        update = subscription.next() => {
                            let Update::ConnectorLogin(update) = update? else {
                                return Err(Fault::invalid("connector login update expected"));
                            };
                            if let LoginState::Browser { url, .. } = &update.state
                                && let Some(listener) = callback.take()
                            {
                                let (reply, returned) = tokio::sync::oneshot::channel();
                                browser_reply = Some(reply);
                                callback_task = Some(Box::pin(listener.receive(
                                    url.clone(), crate::oauth::page::response(returned), stop.clone(),
                                )));
                            }
                            let _ = sender.send(Message::State(update.state.clone()));
                            match update.state {
                                LoginState::Connected(_) => {
                                    if let Some(reply) = browser_reply.take() {
                                        let _ = reply.send(crate::oauth::page::Outcome::Connected);
                                    }
                                    return Ok(());
                                }
                                LoginState::Failed(error) => {
                                    if let Some(reply) = browser_reply.take() {
                                        let _ = reply.send(crate::oauth::page::Outcome::Failed);
                                    }
                                    return Err(error);
                                }
                                LoginState::Cancelled => {
                                    if let Some(reply) = browser_reply.take() {
                                        let _ = reply.send(crate::oauth::page::Outcome::Cancelled);
                                    }
                                    return Ok(());
                                }
                                _ => (),
                            }
                        }
                    }
                }
            };
            if let Err(error) = operation.await {
                let _ = sender.send(Message::Failed(error));
            }
        });
        let keep_alive = cx.entity();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let _keep_alive = keep_alive;
            let _job = job;
            while let Some(message) = receiver.recv().await {
                if owner.update_in(cx, |this, window, cx| {
                    if this.closed || observed.is_cancelled() {
                        return;
                    }
                    match message {
                        Message::Attempt(attempt) => this.attempt = Some(attempt),
                        Message::State(LoginState::Connected(connection)) => {
                            this.request = None;
                            this.callback = None;
                            this.pending = false;
                            let title = this.kind()
                                .map(|kind| super::super::title(&kind.declaration.display))
                                .unwrap_or_default();
                            this.closed = true;
                            window.close_dialog(cx);
                            crate::ui::authorization::connected(&title, connection.identity.as_ref().map(|identity| identity.name.as_str()), window, cx);
                        }
                        Message::State(state) => {
                            this.pending = state.active();
                            if matches!(&state, LoginState::Failed(_) | LoginState::Cancelled) {
                                this.request = None;
                                this.callback = None;
                            }
                            if let LoginState::Browser { url, .. } = &state
                                && !matches!(&this.flow, Some(LoginState::Browser { url: previous, .. }) if previous == url)
                            {
                                cx.open_url(url);
                            }
                            this.flow = Some(state);
                        }
                        Message::Saved(_connection) => {
                            this.request = None;
                            this.callback = None;
                            this.pending = false;
                            this.closed = true;
                            window.close_dialog(cx);
                            crate::feedback::toast(window, tr("connector_saved"), notification::Notification::success(tr("connector_saved")), cx);
                        }
                        Message::Failed(error) => {
                            if !matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable) {
                                this.request = None;
                                this.callback = None;
                            }
                            this.pending = false;
                            this.flow = interactive.then_some(LoginState::Failed(error.clone()));
                            failure(error, window, cx);
                        }
                    }
                    cx.notify();
                }).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(super) fn authorization(&self, cx: &mut Context<Self>) -> AnyElement {
        let flow = match &self.flow {
            Some(LoginState::Device {
                code,
                url,
                expires_at_ms,
            }) => Flow::Device {
                code: code.clone().into(),
                url: url.clone().into(),
                expires_at_ms: Some(*expires_at_ms),
            },
            Some(LoginState::Browser { url, .. }) => Flow::Browser {
                url: url.clone().into(),
            },
            Some(LoginState::Failed(error)) if error.code == ErrorCode::Expired => Flow::Expired {
                code: None,
                url: None,
            },
            Some(LoginState::Failed(_)) => Flow::Failed {
                code: None,
                url: None,
            },
            _ => Flow::Starting,
        };
        let owner = cx.entity();
        let title = self
            .kind()
            .map(|kind| super::super::title(&kind.declaration.display))
            .unwrap_or_default();
        Authorization::new(
            crate::plugins::emblem::surface(&title, None, px(TILE))
                .child(Icon::new(gpui_kit::assets::IconName::Plug).size_6()),
            title,
            flow,
        )
        .busy(matches!(
            self.flow,
            Some(LoginState::Starting | LoginState::Exchanging)
        ))
        .on_refresh(move |window, cx| owner.update(cx, |this, cx| this.submit(false, window, cx)))
        .into_any_element()
    }
}
