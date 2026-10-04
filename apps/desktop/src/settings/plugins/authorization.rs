use super::*;
use crate::settings::providers::Binding;
use crate::theme::DialogStyle as _;
use gpui_kit::component::input::{Input, InputState};
use sailry_client::mcp_login::View;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Output, Request,
    plugin::{
        Reference,
        authorization::{Attempt, State},
    },
};
mod callback;
#[cfg(test)]
#[path = "../../../../../crates/node-runtime/tests/support/mcp_oauth.rs"]
#[allow(dead_code)]
mod issuer;
#[cfg(test)]
mod tests;
mod view;

pub(in crate::settings) fn open(
    binding: Binding,
    package: Reference,
    server: String,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Login> {
    let login = cx.new(|cx| Login {
        client_id: cx.new(|cx| InputState::new(window, cx).placeholder(tr("mcp_oauth_client_id"))),
        binding,
        package,
        server,
        configured: None,
        view: View::default(),
        request: None,
        pending: false,
        closing: false,
        closed: false,
        error: None,
        callback: None,
        stop: CancellationToken::new(),
        task: None,
        observer: None,
        receiver: None,
    });
    login.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    login.update(cx, |login, cx| {
        login.prepare(Command::ReadMcpAuthorization {
            package: login.package.clone(),
            server: login.server.clone(),
        });
        login.send(window, cx);
    });
    window.open_dialog(cx, {
        let login = login.clone();
        move |dialog, window, _| {
            let cancel = login.clone();
            let close = login.clone();
            dialog
                .form_title(tr("mcp_oauth_title"))
                .w((window.viewport_size().width - px(48.)).min(px(520.)))
                .on_ok(|_, _, _| false)
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |login, cx| login.cancel(window, cx))
                })
                .on_close(move |_, _, cx| {
                    close.update(cx, |login, _| {
                        login.closed = true;
                        login.stop.cancel();
                    })
                })
                .child(login.clone())
        }
    });
    login
}

pub(in crate::settings) struct Login {
    binding: Binding,
    package: Reference,
    server: String,
    client_id: Entity<InputState>,
    configured: Option<bool>,
    view: View,
    request: Option<Request>,
    pending: bool,
    closing: bool,
    closed: bool,
    error: Option<&'static str>,
    callback: Option<callback::Callback>,
    stop: CancellationToken,
    task: Option<Task<()>>,
    observer: Option<Task<()>>,
    receiver: Option<Task<()>>,
}

impl Drop for Login {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Login {
    fn state(&self) -> Option<&State> {
        self.view.update.as_ref().map(|update| &update.state)
    }
    fn active(&self) -> bool {
        self.state().is_some_and(State::active)
    }
    fn prepare(&mut self, command: Command) {
        self.request = Some(self.binding.client.prepare(command));
    }

    fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.active() || self.request.is_some() {
            return;
        }
        let callback = match callback::Callback::bind() {
            Ok(callback) => callback,
            Err(_) => {
                self.error = Some("mcp_oauth_callback_failed");
                cx.notify();
                return;
            }
        };
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.view = View::default();
        self.receiver = None;
        let id = self.client_id.read(cx).value().trim().to_owned();
        self.prepare(Command::BeginMcpLogin {
            package: self.package.clone(),
            server: self.server.clone(),
            redirect: callback.redirect.clone(),
            client_id: (!id.is_empty()).then_some(id),
        });
        self.callback = Some(callback);
        self.send(window, cx);
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let Some(request) = self.request.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let command = request.command.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |login, window, cx| {
                login.pending = false;
                if login.closed {
                    return;
                }
                let completed = matches!(&result, Ok(Ok(_)));
                match result {
                    Ok(Ok(output)) => {
                        login.request = None;
                        match (command, output) {
                            (
                                Command::ReadMcpAuthorization { .. },
                                Output::McpAuthorization(status),
                            ) => login.configured = Some(status.configured),
                            (Command::BeginMcpLogin { .. }, Output::McpLogin(attempt)) => {
                                login.observe(attempt, window, cx)
                            }
                            (Command::CompleteMcpLogin { .. }, Output::McpLogin(_)) => {}
                            (Command::CancelMcpLogin { .. }, Output::McpLogin(_)) => {
                                login.closed = true;
                                login.stop.cancel();
                                window.close_dialog(cx);
                            }
                            (
                                Command::RevokeMcpAuthorization { .. },
                                Output::McpAuthorization(status),
                            ) => {
                                login.package = status.package;
                                login.configured = Some(status.configured);
                                login.view = View::default();
                            }
                            _ => login.error = Some("plugins_failed"),
                        }
                    }
                    Ok(Err(error)) => {
                        if !live::uncertain(&error) {
                            login.request = None;
                        }
                        login.error = Some(live::error_key(&error));
                    }
                    _ => login.error = Some("plugins_unknown"),
                }
                if completed
                    && login.closing
                    && !login.closed
                    && login.request.is_none()
                    && login.cancel(window, cx)
                {
                    window.close_dialog(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn observe(&mut self, attempt: Attempt, window: &mut Window, cx: &mut Context<Self>) {
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let (client, stop) = (self.binding.client.clone(), self.stop.clone());
        // Preserve the accepted attempt while the shared observer connects.
        self.view.update = Some(sailry_protocol::plugin::authorization::Update {
            attempt: attempt.clone(),
            revision: 1,
            state: State::Starting,
        });
        self.binding.runtime.spawn(async move {
            let _ = client.watch_mcp_login(attempt, sender, stop).await;
        });
        self.observer = Some(cx.spawn_in(window, async move |owner, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if owner
                    .update_in(cx, |login, window, cx| {
                        if login.closed {
                            return;
                        }
                        if view.update.is_some() {
                            login.view = view.clone();
                        }
                        if let Some(error) = &view.error {
                            login.error = Some(live::error_key(error));
                        }
                        match login.state().cloned() {
                            Some(State::Pending { url, .. }) if !login.closing => {
                                login.receive(url, window, cx)
                            }
                            Some(State::Connected(package)) => {
                                login.package = package;
                                login.configured = Some(true);
                                login.stop.cancel();
                            }
                            Some(State::Failed(_)) => {
                                login.error = Some("mcp_oauth_failed");
                                login.stop.cancel();
                            }
                            Some(State::Cancelled) => {
                                login.stop.cancel();
                            }
                            _ => {}
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        }));
    }

    fn receive(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        let stop = self.stop.clone();
        let message = tr("mcp_oauth_return").to_string();
        let job = self
            .binding
            .runtime
            .spawn(callback.receive(url, message, stop));
        self.receiver = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |login, window, cx| {
                if login.closed || login.closing || !login.active() {
                    return;
                }
                match result {
                    Ok(Ok(callback)) => {
                        let attempt = login.view.update.as_ref().unwrap().attempt.id;
                        login.prepare(Command::CompleteMcpLogin { attempt, callback });
                        login.send(window, cx);
                    }
                    _ => {
                        login.error = Some("mcp_oauth_callback_failed");
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.closing = true;
        if self.pending {
            cx.notify();
            return false;
        }
        if self.request.is_some() {
            self.send(window, cx);
            return false;
        }
        if self.active() {
            let attempt = self.view.update.as_ref().unwrap().attempt.id;
            self.prepare(Command::CancelMcpLogin { attempt });
            self.send(window, cx);
            return false;
        }
        self.closed = true;
        self.stop.cancel();
        true
    }
}
