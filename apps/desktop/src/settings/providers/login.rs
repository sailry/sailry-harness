use super::{Workspace, live::Binding};
use crate::theme::DialogStyle as _;
use crate::tr;
use gpui_kit::{component::*, prelude::FluentBuilder, *};
use sailry_client::login::View;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, Request,
    conversation::{
        Provider,
        login::{Attempt, State},
    },
};

pub(super) mod revoke;
mod view;
#[cfg(test)]
use super::authorization_support as support;
#[cfg(test)]
mod tests;

pub(super) fn binding(
    owner: &Entity<Workspace>,
    id: usize,
    cx: &App,
) -> Option<(Binding, Provider)> {
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
    let login = cx.new(|cx| {
        let mut login = Login::new(binding, provider);
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
            .form_title(tr("provider_connect"))
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .on_ok(|_, _, _| false)
            .on_cancel(move |_, window, cx| cancel.update(cx, |login, cx| login.cancel(window, cx)))
            .on_close(move |_, _, cx| {
                close.update(cx, |login, _| {
                    login.closed = true;
                    login.stop.cancel();
                })
            })
            .child(login.clone())
            .footer(login.read(cx).footer(&login, cx))
    });
    Some(result)
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
}

impl Login {
    fn new(binding: Binding, provider: Provider) -> Self {
        let request = binding.client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
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
            fresh: false,
            stop: CancellationToken::new(),
            task: None,
            observer: None,
        }
    }

    fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        if self.fresh {
            self.attempt = None;
            self.request = self.binding.client.prepare(Command::BeginProviderLogin {
                provider: self.provider.id,
                expected_revision: self.provider.revision,
            });
            self.cancellation = self.binding.client.prepare(Command::CancelProviderLogin {
                attempt: self.request.id,
            });
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
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |login, window, cx| {
                login.pending = false;
                if login.closed {
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
                        login.closed = true;
                        login.error = None;
                        login.report("provider_login_cancelled", window, cx);
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
                        if login.closed {
                            return;
                        }
                        login.view = view;
                        match login.state() {
                            Some(State::Connected) => {
                                login.report("provider_login_connected", window, cx);
                            }
                            Some(State::Cancelled) => {
                                login.report("provider_login_cancelled", window, cx);
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
                            login.error = Some(error_key(error));
                            login.fresh = true;
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

    fn state(&self) -> Option<&State> {
        self.view.update.as_ref().map(|update| &update.state)
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
        if key == "provider_login_connected" {
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

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.closed
            || self.state().is_some_and(|state| !state.active())
            || self.error == Some("provider_login_changed")
            || (self.attempt.is_none() && self.fresh)
        {
            return true;
        }
        self.cancelling = true;
        if self.pending {
            cx.notify();
            return false;
        }
        if self.attempt.is_none() {
            // Resolve the original admission before cancelling: independent Link
            // streams may otherwise deliver cancellation before a delayed begin.
            self.begin(window, cx);
            return false;
        }
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let request = self.cancellation.clone();
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
                    login.stop.cancel();
                    // Presentation completion of the explicit cancel command;
                    // the shared observer remains authoritative for login updates.
                    login.error = None;
                    login.closed = true;
                    login.report("provider_login_cancelled", window, cx);
                } else {
                    login.error = Some("provider_login_cancel_unknown");
                }
                cx.notify();
            });
        }));
        cx.notify();
        false
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.stop.cancel();
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
