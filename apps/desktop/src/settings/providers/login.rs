use super::Workspace;
use super::live::Binding;
use crate::theme::DialogStyle as _;
use crate::tr;
use gpui_kit::{component::*, prelude::FluentBuilder, *};
use sailry_client::login::View;
use sailry_client::oauth::Callback;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, Request,
    conversation::{
        Provider,
        login::{Attempt, State},
    },
};

mod browser;
mod declaration;
#[cfg(test)]
mod tests;
mod view;

fn binding(owner: &Entity<Workspace>, id: usize, cx: &App) -> Option<(Binding, Provider)> {
    let live = owner.read(cx).provider_link.as_ref()?;
    Some((live.binding.clone(), live.providers.get(&id)?.clone()))
}

pub(super) fn open(
    owner: Entity<Workspace>,
    id: usize,
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<Login>> {
    let (binding, provider) = binding(&owner, id, cx)?;
    Some(open_for(binding, provider, window, cx))
}

pub(super) fn open_for(
    binding: Binding,
    provider: Provider,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Login> {
    let login = cx.new(|cx| {
        let mut login = Login::new(binding, provider, window, cx);
        login.begin(window, cx);
        login
    });
    login.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| view.error.into_iter().collect());
    });
    let result = login.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let cancel = login.clone();
        let close = login.clone();
        dialog
            .form_title(tr("provider_connect"), cx)
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .on_ok(|_, _, _| false)
            .on_cancel(move |_, window, cx| cancel.update(cx, |login, cx| login.cancel(window, cx)))
            .on_close(move |_, _, cx| {
                close.update(cx, |login, _| {
                    login.closed = true;
                    if !login.cancelling {
                        login.stop.cancel();
                        login.callback_stop.cancel();
                        login.callback = None;
                    }
                })
            })
            .child(login.clone())
            .when(
                login.read(cx).provider.authentication != sailry_protocol::Authentication::ChatGpt,
                |dialog| dialog.footer(login.read(cx).footer(&login, cx)),
            )
    });
    result
}

pub(super) struct Login {
    binding: Binding,
    provider: Provider,
    request: Request,
    attempt: Option<Attempt>,
    cancellation: Request,
    view: View,
    pending: bool,
    cancelling: bool,
    closed: bool,
    error: Option<&'static str>,
    presented: Option<(sailry_protocol::RequestId, &'static str)>,
    fresh: bool,
    stop: CancellationToken,
    task: Option<Task<()>>,
    observer: Option<Task<()>>,
    callback: Option<Callback>,
    presentation: Option<sailry_protocol::plugin::model_auth::Presentation>,
    declaration_task: Option<Task<()>>,
    callback_stop: CancellationToken,
    browser_reply: Option<tokio::sync::oneshot::Sender<crate::oauth::page::Outcome>>,
    completion: Option<Request>,
    receiver: Option<Task<()>>,
    opened: Option<sailry_protocol::RequestId>,
    manual: Entity<input::InputState>,
}

pub(super) enum Event {
    Connected,
}

impl EventEmitter<Event> for Login {}

impl Login {
    fn new(
        binding: Binding,
        provider: Provider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let request = binding.client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
            redirect: None,
        });
        let cancellation = binding.client.prepare(Command::CancelProviderLogin {
            attempt: request.id,
        });
        Self {
            binding,
            provider,
            request,
            attempt: None,
            cancellation,
            view: View::default(),
            pending: false,
            cancelling: false,
            closed: false,
            error: None,
            presented: None,
            fresh: true,
            stop: CancellationToken::new(),
            task: None,
            observer: None,
            callback: None,
            presentation: None,
            declaration_task: None,
            callback_stop: CancellationToken::new(),
            browser_reply: None,
            completion: None,
            receiver: None,
            opened: None,
            manual: cx.new(|cx| {
                input::InputState::new(window, cx).placeholder(tr("provider_login_callback"))
            }),
        }
    }

    fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || (self.closed && !self.cancelling) {
            return;
        }
        if self.provider.authentication == sailry_protocol::Authentication::Plugin
            && self.presentation.is_none()
        {
            self.load_declaration(window, cx);
            return;
        }
        if self.completion.is_some() && !self.fresh && !self.cancelling {
            self.complete(window, cx);
            return;
        }
        if self.fresh {
            let callback = if let Some((path, required)) =
                declaration::listener(self.provider.authentication, self.presentation)
            {
                match Callback::bind(path) {
                    Ok(callback) => Some(callback),
                    Err(_) if required => {
                        self.error = Some("provider_login_failed");
                        cx.notify();
                        return;
                    }
                    Err(_) => None,
                }
            } else {
                None
            };
            self.attempt = None;
            self.request = self.binding.client.prepare(Command::BeginProviderLogin {
                provider: self.provider.id,
                expected_revision: self.provider.revision,
                redirect: callback.as_ref().map(|callback| callback.redirect.clone()),
            });
            self.cancellation = self.binding.client.prepare(Command::CancelProviderLogin {
                attempt: self.request.id,
            });
            self.callback = callback;
            self.browser_reply = None;
            self.callback_stop.cancel();
            self.callback_stop = CancellationToken::new();
            self.receiver = None;
            self.completion = None;
            self.manual
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.fresh = false;
        }
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.observer = None;
        self.view = View::default();
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let request = self.request.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        // A dismissed dialog must still resolve admission before cancelling it.
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
                        if attempt.id == login.request.id
                            && attempt.provider == login.provider.id =>
                    {
                        login.attempt = Some(attempt.clone());
                        login.observe(attempt, window, cx);
                    }
                    Ok(Err(error)) => {
                        login.error = Some(error_key(&error));
                        login.fresh = !matches!(
                            error.code,
                            ErrorCode::Unavailable
                                | ErrorCode::OutcomeUnknown
                                | ErrorCode::Internal
                        );
                    }
                    _ => login.error = Some("provider_login_unknown"),
                }
                if login.cancelling {
                    if login.attempt.is_some() {
                        login.cancel(window, cx);
                    } else if login.fresh {
                        login.cancelling = false;
                        login.closed = true;
                        login.error = None;
                        login.stop.cancel();
                        login.callback_stop.cancel();
                        login.callback = None;
                    } else {
                        login.error = Some("provider_login_cancel_unknown");
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn observe(&mut self, attempt: Attempt, window: &mut Window, cx: &mut Context<Self>) {
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        self.binding.runtime.spawn(async move {
            let _ = client.watch_login(attempt, sender, stop).await;
        });
        self.observer = Some(cx.spawn_in(window, async move |owner, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if owner
                    .update_in(cx, |login, window, cx| {
                        if login.closed && !login.cancelling {
                            return;
                        }
                        login.view = view;
                        match login.state().cloned() {
                            Some(State::Browser {
                                authorization_url, ..
                            }) if !login.cancelling => {
                                login.receive(authorization_url.clone(), window, cx);
                                login.open_browser(&authorization_url, cx);
                            }
                            Some(State::Pending {
                                verification_url, ..
                            }) if !login.cancelling => {
                                login.open_browser(&verification_url, cx);
                            }
                            Some(State::Connected) => {
                                login
                                    .manual
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                login.reply(crate::oauth::page::Outcome::Connected);
                                login.completion = None;
                                let key = if login
                                    .view
                                    .update
                                    .as_ref()
                                    .is_some_and(|update| update.model_error.is_some())
                                {
                                    "provider_login_models_failed"
                                } else {
                                    "provider_login_connected"
                                };
                                let visible = !login.closed;
                                login.report(key, window, cx);
                                login.closed = true;
                                login.stop.cancel();
                                login.callback_stop.cancel();
                                login.callback = None;
                                cx.emit(Event::Connected);
                                if visible {
                                    window.close_dialog(cx);
                                }
                            }
                            Some(State::Cancelled) => {
                                login
                                    .manual
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                login.reply(crate::oauth::page::Outcome::Cancelled);
                                login.completion = None;
                                if !login.closed {
                                    window.close_dialog(cx);
                                }
                                login.closed = true;
                                login.stop.cancel();
                                login.callback_stop.cancel();
                                login.callback = None;
                            }
                            _ => {}
                        }
                        if !login.cancelling {
                            login.error = None;
                            login.fresh = false;
                        }
                        if login.state().is_some_and(|state| !state.active()) {
                            login.cancelling = false;
                            login.error = None;
                        }
                        if let Some(State::Failed(error)) = login.state() {
                            let key = error_key(error);
                            login.reply(crate::oauth::page::Outcome::Failed);
                            login.completion = None;
                            login.error = Some(key);
                            login.fresh = true;
                            login.stop.cancel();
                            login.callback_stop.cancel();
                            login.callback = None;
                        } else if let Some(error) = &login.view.error {
                            login.error = Some(error_key(error));
                            login.fresh = error.code == ErrorCode::NotFound;
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

    pub(super) fn state(&self) -> Option<&State> {
        self.view.update.as_ref().map(|update| &update.state)
    }

    fn reply(&mut self, outcome: crate::oauth::page::Outcome) {
        if let Some(sender) = self.browser_reply.take() {
            let _ = sender.send(outcome);
        }
    }

    fn open_browser(&mut self, url: &str, cx: &mut App) {
        if self.opened == Some(self.request.id) {
            return;
        }
        self.opened = Some(self.request.id);
        cx.open_url(url);
    }

    fn expired(&self) -> bool {
        !self.closed && self.error == Some("provider_login_expired")
    }

    fn report(&mut self, key: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let identity = (self.request.id, key);
        if self.presented == Some(identity) {
            return;
        }
        self.presented = Some(identity);
        if key == "provider_login_models_failed" {
            crate::feedback::error("", &tr(key), window, cx);
        } else if key == "provider_login_connected" {
            crate::feedback::status(
                window,
                tr(key),
                notification::NotificationType::Success,
                notification::Notification::success(tr(key)),
                cx,
            );
        } else {
            crate::feedback::info("", &tr(key), window, cx);
        }
    }

    pub(super) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.manual
            .update(cx, |input, cx| input.set_value("", window, cx));
        if (self.closed && !self.cancelling)
            || self.state().is_some_and(|state| !state.active())
            || self.error == Some("provider_login_changed")
            || (self.attempt.is_none() && self.fresh)
        {
            let outcome = match self.state() {
                Some(State::Connected) => crate::oauth::page::Outcome::Connected,
                Some(State::Cancelled) => crate::oauth::page::Outcome::Cancelled,
                Some(State::Failed(_)) => crate::oauth::page::Outcome::Failed,
                _ => crate::oauth::page::Outcome::Pending,
            };
            self.reply(outcome);
            self.closed = true;
            self.cancelling = false;
            self.error = None;
            self.stop.cancel();
            self.callback_stop.cancel();
            self.callback = None;
            self.completion = None;
            cx.notify();
            return true;
        }
        self.callback_stop.cancel();
        self.callback = None;
        self.cancelling = true;
        if self.pending {
            cx.notify();
            return true;
        }
        if self.attempt.is_none() {
            // Resolve the original admission before cancelling: independent Link
            // streams may otherwise deliver cancellation before a delayed begin.
            self.begin(window, cx);
            return true;
        }
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let request = self.cancellation.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        let keep_alive = cx.entity();
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let _keep_alive = keep_alive;
            let result = job.await;
            let _ = owner.update_in(cx, |login, _, cx| {
                login.pending = false;
                if login.closed && !login.cancelling {
                    return;
                }
                if login.state().is_some_and(|state| !state.active()) {
                    login.cancelling = false;
                    login.error = None;
                    cx.notify();
                    return;
                }
                let cancelled = match result {
                    Ok(Ok(Output::ProviderLogin(attempt))) => {
                        login.attempt.as_ref() == Some(&attempt)
                    }
                    Ok(Err(Fault {
                        code: ErrorCode::NotFound,
                        ..
                    })) => true,
                    _ => false,
                };
                if cancelled {
                    login.reply(crate::oauth::page::Outcome::Cancelled);
                    login.stop.cancel();
                    login.callback_stop.cancel();
                    login.callback = None;
                    // Presentation completion of the explicit cancel command;
                    // the shared observer remains authoritative for login updates.
                    login.error = None;
                    login.closed = true;
                    login.cancelling = false;
                } else {
                    login.error = Some("provider_login_cancel_unknown");
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.stop.cancel();
        self.callback_stop.cancel();
    }
}

fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "provider_login_changed",
        ErrorCode::NotFound | ErrorCode::Expired => "provider_login_expired",
        ErrorCode::Busy => "provider_login_busy",
        ErrorCode::OutcomeUnknown => "provider_login_unknown",
        _ => "provider_login_failed",
    }
}
