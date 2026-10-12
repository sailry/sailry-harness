//! Kit has no protected QR or credential-issuing user-action binding for scripts.
//! Packages compose their page; this native adapter retains private attempts and
//! uses Client's existing login projection, Node receipts and schema settings.
use super::*;
use gpui_kit::{
    Context,
    base::Disableable as _,
    component::{
        Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder as _,
    *,
};
use sailry_client::plugin_login;
use sailry_protocol::{
    ErrorCode, Fault, Output, RequestOutcome,
    plugin::{self, authorization::State, oauth::Attempt, registration::Declaration},
};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, watch};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
enum Action {
    Begin { replace: bool },
    Cancel,
    Open,
}

#[derive(Clone, Copy)]
enum Continuation {
    Observe,
    Cancel,
    Replace,
}

struct Controller {
    host: Arc<Host>,
    declaration: Option<Declaration>,
    configured: bool,
    phase: &'static str,
    fault: Option<&'static str>,
    pending: bool,
    closed: bool,
    request: Option<Request>,
    attempt: Option<Attempt>,
    view: plugin_login::View,
    bitmap: Option<(String, crate::plugins::cards::qr::Bitmap)>,
    updates: watch::Sender<Value>,
    sequence: u64,
    stop: CancellationToken,
    task: Option<Task<()>>,
    observer: Option<Task<()>>,
    lifecycle: Option<Task<()>>,
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.close();
    }
}

impl Host {
    pub(in crate::plugins) fn registration_module(
        self: &Arc<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> HostModule {
        let owner = cx.new(|_| Controller {
            host: self.clone(),
            declaration: None,
            configured: false,
            phase: "idle",
            fault: None,
            pending: false,
            closed: false,
            request: None,
            attempt: None,
            view: Default::default(),
            bitmap: None,
            updates: watch::channel(json!({})).0,
            sequence: 0,
            stop: self.stop.child_token(),
            task: None,
            observer: None,
            lifecycle: None,
        });
        owner.update(cx, |state, cx| {
            state.publish(cx);
            if state.host.context().controller_settings() {
                state.load(window, cx);
            }
            let stop = state.host.stop_token();
            state.lifecycle = Some(cx.spawn_in(window, async move |owner, cx| {
                stop.cancelled().await;
                let _ = owner.update_in(cx, |state, _, cx| {
                    state.close();
                    state.publish(cx);
                });
            }));
        });
        let (read, buttons, qr) = (owner.clone(), owner.clone(), owner);
        let host = self.clone();
        let capacity = Arc::new(Semaphore::new(1));
        HostModule::new("sailry/registration")
            .async_function("nextRegistration", move |args| {
                host.check()?;
                if !host.context().controller_settings() {
                    return Err(HostError::new("registration requires the settings surface"));
                }
                let seen = args.get(0).and_then(HostValue::as_str).unwrap_or_default().to_owned();
                let mut receiver = gpui_shell::with_current_app(|cx| read.read(cx).updates.subscribe())
                    .ok_or_else(|| HostError::new("registration observation requires an active view"))?;
                let permit = capacity.clone().try_acquire_owned()
                    .map_err(|_| HostError::new("a registration observation is already pending"))?;
                let stop = host.stop_token();
                Ok(async move {
                    let _permit = permit;
                    loop {
                        if stop.is_cancelled() { return Err(HostError::new("plugin view is closed")); }
                        let value = receiver.borrow_and_update().clone();
                        if value.get("cursor").and_then(Value::as_str) != Some(seen.as_str()) {
                            return sdk::values::encode(value);
                        }
                        tokio::select! {
                            _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                            result = receiver.changed() => if result.is_err() {
                                return Err(HostError::new("registration observation ended"));
                            },
                        }
                    }
                })
            })
            .component("RegistrationButton", move |args, _, cx| {
                let label = args.props().get("label").and_then(HostValue::as_str).unwrap_or_default().to_owned();
                let action = match args.props().get("action").and_then(HostValue::as_str) {
                    Some("begin") => Some(Action::Begin { replace:
                        args.props().get("replace_pending").and_then(HostValue::as_bool) == Some(true) }),
                    Some("cancel") => Some(Action::Cancel), Some("open") => Some(Action::Open), _ => None,
                };
                let state = buttons.read(cx);
                let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true)
                    || state.closed || state.pending || state.declaration.is_none()
                    || state.host.check().is_err() || !state.host.context().controller_settings()
                    || action.is_none()
                    || matches!(action, Some(Action::Open)) && state.url().is_none();
                let primary = args.props().get("primary").and_then(HostValue::as_bool) == Some(true);
                let id = args.id().to_owned();
                let owner = buttons.clone();
                Button::new(SharedString::from(id.clone()))
                    .map(|button| if primary { button.primary() } else { button.ghost() })
                    .small().label(label.clone()).accessibility_label(label).disabled(disabled)
                    .debug_selector(move || id.clone())
                    .on_click(move |_, window, cx| {
                        if let Some(action) = action {
                            owner.update(cx, |state, cx| state.action(action, window, cx));
                        }
                    }).into_any_element()
            })
            .component("RegistrationQR", move |args, _, cx| {
                qr.update(cx, |state, _| {
                    let Some(url) = state.url().map(str::to_owned) else {
                        state.bitmap = None;
                        return div().into_any_element();
                    };
                    if state.bitmap.as_ref().is_none_or(|(previous, _)| *previous != url) {
                        let Ok(bitmap) = crate::plugins::cards::qr::render(&url) else {
                            return div().into_any_element();
                        };
                        state.bitmap = Some((url, bitmap));
                    }
                    let bitmap = &state.bitmap.as_ref().unwrap().1;
                    let id = args.id().to_owned();
                    img(bitmap.image.clone()).w(px(bitmap.width as f32)).h(px(bitmap.width as f32))
                        .max_w_full().object_fit(ObjectFit::Contain)
                        .debug_selector(move || id.clone()).into_any_element()
                })
            })
            .declarations(include_str!("registration/api.d.ts"))
    }
}

impl Controller {
    fn publish(&mut self, cx: &mut Context<Self>) {
        self.sequence = self.sequence.wrapping_add(1);
        let unresolved = self.request.is_some() && !self.pending;
        self.updates.send_replace(json!({
            "cursor": self.sequence.to_string(),
            "phase": if unresolved { "unconfirmed" } else { self.phase },
            "configured": self.configured, "busy": self.pending,
            "can_open": self.url().is_some(),
            "can_replace": self.attempt.is_some() && self.active(),
            "fault": if unresolved { Some("outcome_unknown") } else { self.fault },
        }));
        cx.notify();
    }

    fn active(&self) -> bool {
        self.view
            .update
            .as_ref()
            .is_some_and(|update| update.state.active())
    }

    fn url(&self) -> Option<&str> {
        if self.closed || self.pending || !self.view.connected || self.host.check().is_err() {
            return None;
        }
        let State::Pending { url, expires_at_ms } = &self.view.update.as_ref()?.state else {
            return None;
        };
        (*expires_at_ms > chrono::Utc::now().timestamp_millis().max(0) as u64).then_some(url)
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || self.pending || !self.host.context().controller_settings() {
            return;
        }
        self.pending = true;
        let (host, stop) = (self.host.clone(), self.stop.clone());
        let job = self.host.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = configuration(&host) => Some(result),
            }
        });
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |state, _, cx| {
                if state.closed {
                    return;
                }
                state.pending = false;
                match result {
                    Ok(Some(Ok((declaration, configured)))) => {
                        state.declaration = Some(declaration);
                        state.configured = configured;
                        state.fault = None;
                    }
                    _ => {
                        state.phase = "unavailable";
                        state.fault = Some("unavailable");
                    }
                }
                state.publish(cx);
            });
        }));
        self.publish(cx);
    }

    fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed
            || self.pending
            || self.host.check().is_err()
            || !self.host.context().controller_settings()
            || self.declaration.is_none()
        {
            return;
        }
        match action {
            Action::Open => {
                if let Some(url) = self.url() {
                    cx.open_url(url);
                }
            }
            Action::Cancel => self.cancel(false, window, cx),
            Action::Begin { replace } => {
                // A user checks the original uncertain receipt; no new identity is created.
                if self.request.is_some() {
                    self.send(true, Continuation::Observe, window, cx);
                } else if self.phase == "unconfirmed" {
                    if let Some(attempt) = self.attempt.clone() {
                        if let Some(State::Connected(reference)) =
                            self.view.update.as_ref().map(|update| &update.state)
                        {
                            self.adopt(reference.clone(), window, cx);
                        } else {
                            self.stop.cancel();
                            self.stop = self.host.stop.child_token();
                            self.observe(attempt, window, cx);
                        }
                    }
                } else if self.attempt.is_some() && self.active() {
                    if replace {
                        self.cancel(true, window, cx);
                    }
                } else {
                    self.begin(window, cx);
                }
            }
        }
    }

    fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.settings_readable().is_err() {
            return;
        }
        let context = self.host.context();
        self.stop.cancel();
        self.stop = self.host.stop.child_token();
        self.observer = None;
        self.attempt = None;
        self.view = Default::default();
        self.bitmap = None;
        self.fault = None;
        self.phase = "starting";
        self.request = Some(
            self.host
                .client
                .prepare(Command::BeginPluginRegistration {
                    package: context.package.clone(),
                })
                .with_plugin(context),
        );
        self.send(false, Continuation::Observe, window, cx);
    }

    fn cancel(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.request.is_some() {
            self.send(true, Continuation::Cancel, window, cx);
            return;
        }
        let Some(attempt) = self.attempt.as_ref().filter(|_| self.active()) else {
            return;
        };
        // This native owner retains its original caller and attempt even if settings change.
        self.request = Some(self.host.client.prepare(Command::CancelPluginLogin {
            attempt: attempt.id,
        }));
        self.send(
            false,
            if replace {
                Continuation::Replace
            } else {
                Continuation::Observe
            },
            window,
            cx,
        );
    }

    fn send(
        &mut self,
        inspect: bool,
        next: Continuation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = self.request.clone() else {
            return;
        };
        self.pending = true;
        self.fault = None;
        let command = request.command.clone();
        let client = self.host.client.clone();
        let (delivered, result) = tokio::sync::oneshot::channel();
        let (accepted, ownership) = tokio::sync::oneshot::channel();
        self.host.runtime.spawn(async move {
            let result = if inspect {
                match client.outcome(&request).await {
                    Ok(RequestOutcome::Completed(result)) => (*result, true),
                    Ok(_) => (
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "registration outcome is unknown",
                        )),
                        false,
                    ),
                    Err(error) => (Err(error), false),
                }
            } else {
                (client.execute(request.clone()).await, false)
            };
            let owned = match &result.0 {
                Ok(Output::PluginLogin(attempt)) if original(&request, attempt) => {
                    Some(attempt.clone())
                }
                _ => None,
            };
            let unresolved = matches!(&result, (Err(fault), false) if uncertain(fault.code));
            let _ = delivered.send(result);
            if !matches!(ownership.await, Ok(true)) {
                if let Some(attempt) = owned {
                    cancel_owned(&client, attempt).await;
                } else if unresolved {
                    cleanup_request(&client, request).await;
                }
            }
        });
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = result.await;
            let received = owner.update_in(cx, |state, window, cx| {
                if state.closed {
                    return false;
                }
                state.pending = false;
                match result {
                    Ok((Ok(Output::PluginLogin(attempt)), _))
                        if state.matches(&command, &attempt) =>
                    {
                        state.request = None;
                        match command {
                            Command::BeginPluginRegistration { .. } => {
                                state.observe(attempt, window, cx);
                                if matches!(next, Continuation::Cancel) {
                                    state.cancel(false, window, cx);
                                }
                            }
                            Command::CancelPluginLogin { .. } => {
                                state.stop.cancel();
                                state.observer = None;
                                state.attempt = None;
                                state.view = Default::default();
                                state.bitmap = None;
                                state.phase = "cancelled";
                                if matches!(next, Continuation::Replace) {
                                    state.begin(window, cx);
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok((Err(error), confirmed)) if confirmed || !uncertain(error.code) => {
                        state.request = None;
                        state.phase = "failed";
                        state.fault = Some(code(error.code));
                    }
                    _ => {
                        state.phase = "unconfirmed";
                        state.fault = Some("outcome_unknown");
                    }
                }
                state.publish(cx);
                true
            });
            let _ = accepted.send(received.unwrap_or(false));
        }));
        self.publish(cx);
    }

    fn matches(&self, command: &Command, attempt: &Attempt) -> bool {
        match command {
            Command::BeginPluginRegistration { package } => self
                .request
                .as_ref()
                .is_some_and(|request| request.id == attempt.id && *package == attempt.package),
            Command::CancelPluginLogin { attempt: id } => self
                .attempt
                .as_ref()
                .is_some_and(|owned| *id == owned.id && owned == attempt),
            _ => false,
        }
    }

    fn observe(&mut self, attempt: Attempt, window: &mut Window, cx: &mut Context<Self>) {
        self.attempt = Some(attempt.clone());
        if self.stop.is_cancelled() {
            self.stop = self.host.stop.child_token();
        }
        self.view.update = Some(plugin::oauth::Update {
            attempt: attempt.clone(),
            revision: 1,
            state: State::Starting,
        });
        self.phase = "starting";
        let (sender, mut receiver) = watch::channel(plugin_login::View::default());
        let (client, stop) = (self.host.client.clone(), self.stop.clone());
        self.host.runtime.spawn(async move {
            let _ = client.watch_plugin_login(attempt, sender, stop).await;
        });
        self.observer = Some(cx.spawn_in(window, async move |owner, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if owner
                    .update_in(cx, |state, window, cx| {
                        if state.closed {
                            return;
                        }
                        if let Some(update) = &view.update {
                            let changed = state.view.update.as_ref() != Some(update);
                            state.view = view.clone();
                            state.bitmap = None;
                            if view.connected {
                                state.fault = None;
                            }
                            match &update.state {
                                State::Starting => state.phase = "starting",
                                State::Pending { .. } => state.phase = "pending",
                                State::Exchanging => state.phase = "exchanging",
                                State::Cancelled => state.phase = "cancelled",
                                State::Failed(fault) => {
                                    state.phase = "failed";
                                    state.fault = Some(code(fault.code));
                                }
                                State::Connected(reference) if changed => {
                                    state.phase = "exchanging";
                                    state.adopt(reference.clone(), window, cx);
                                }
                                State::Connected(_) => {}
                            }
                        }
                        if let Some(fault) = &view.error {
                            state.phase = "unconfirmed";
                            state.fault = Some(code(fault.code));
                        }
                        state.publish(cx);
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

    fn adopt(&mut self, reference: plugin::Reference, window: &mut Window, cx: &mut Context<Self>) {
        let Some(attempt) = &self.attempt else {
            return;
        };
        if reference.name != attempt.package.name || reference.digest != attempt.package.digest {
            self.phase = "unconfirmed";
            self.fault = Some("outcome_unknown");
            return;
        }
        let host = self.host.clone();
        let job = self.host.runtime.spawn(async move {
            let Output::Plugin(info) = host
                .client
                .execute(host.client.prepare(Command::ReadPlugin {
                    name: reference.name.clone(),
                }))
                .await?
            else {
                return Err(Fault::invalid("plugin information expected"));
            };
            if info.summary.reference() != reference
                || info.summary.digest != host.context().package.digest
            {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "plugin configuration changed",
                ));
            }
            host.observe_settings(&[info.summary], true);
            configuration(&host).await
        });
        self.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |state, _, cx| {
                if state.closed {
                    return;
                }
                match result {
                    Ok(Ok((declaration, configured))) => {
                        state.declaration = Some(declaration);
                        state.configured = configured;
                        state.phase = "connected";
                        state.fault = None;
                    }
                    _ => {
                        state.phase = "unconfirmed";
                        state.fault = Some("outcome_unknown");
                    }
                }
                state.publish(cx);
            });
        }));
    }

    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.stop.cancel();
        self.bitmap = None;
        let request = self.request.take();
        let cancelling = request
            .as_ref()
            .is_some_and(|request| matches!(request.command, Command::CancelPluginLogin { .. }));
        let attempt = self.attempt.take().filter(|_| self.active() && !cancelling);
        let request = request.filter(|_| !self.pending);
        let client = self.host.client.clone();
        if attempt.is_some() || request.is_some() {
            self.host.runtime.spawn(async move {
                if let Some(attempt) = attempt {
                    cancel_owned(&client, attempt).await;
                } else if let Some(request) = request {
                    cleanup_request(&client, request).await;
                }
            });
        }
    }
}

fn original(request: &Request, attempt: &Attempt) -> bool {
    request.id == attempt.id
        && matches!(&request.command,
        Command::BeginPluginRegistration { package } if *package == attempt.package)
}

async fn cancel_owned(client: &Arc<sailry_client::Client>, attempt: Attempt) {
    let _ = client
        .execute(client.prepare(Command::CancelPluginLogin {
            attempt: attempt.id,
        }))
        .await;
}

async fn cleanup_request(client: &Arc<sailry_client::Client>, request: Request) {
    // Retain only the original receipt until admission is known. Never replay a side effect.
    loop {
        match client.outcome(&request).await {
            Ok(RequestOutcome::Completed(result)) => {
                if let Ok(Output::PluginLogin(attempt)) = *result {
                    if original(&request, &attempt) {
                        cancel_owned(client, attempt).await;
                    }
                }
                break;
            }
            Ok(RequestOutcome::Unknown) => break,
            Err(fault) if !uncertain(fault.code) => break,
            _ => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
    }
}

async fn configuration(host: &Arc<Host>) -> Result<(Declaration, bool), Fault> {
    host.check().map_err(|_| unavailable())?;
    host.settings_readable().map_err(|_| unavailable())?;
    let context = host.context();
    if !context.controller_settings() {
        return Err(unavailable());
    }
    let Output::Plugin(info) = host
        .client
        .execute(host.client.prepare(Command::ReadPlugin {
            name: context.package.name.clone(),
        }))
        .await?
    else {
        return Err(unavailable());
    };
    if !info.summary.enabled || info.summary.reference() != context.package {
        return Err(unavailable());
    }
    let declaration = info
        .extension
        .and_then(|extension| extension.registration)
        .filter(Declaration::valid)
        .ok_or_else(unavailable)?;
    let schema = info.settings.ok_or_else(unavailable)?;
    let id = schema
        .properties
        .get(&declaration.client_id)
        .ok_or_else(unavailable)?;
    let secret = schema
        .properties
        .get(&declaration.client_secret)
        .ok_or_else(unavailable)?;
    if id.kind != plugin::settings::Kind::String || id.secret.is_some() || secret.secret.is_none() {
        return Err(unavailable());
    }
    let Output::PluginSettings(settings) = host
        .client
        .execute(
            host.client
                .prepare(Command::ReadPluginSettings {
                    package: context.package.clone(),
                })
                .with_plugin(context),
        )
        .await?
    else {
        return Err(unavailable());
    };
    let configured = settings
        .values
        .get(&declaration.client_id)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
        && settings.configured.contains(&declaration.client_secret);
    Ok((declaration, configured))
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "application registration is unavailable",
    )
}
fn uncertain(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::Unavailable | ErrorCode::OutcomeUnknown | ErrorCode::Busy
    )
}
fn code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::OutcomeUnknown => "outcome_unknown",
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "revision_conflict",
        ErrorCode::PermissionDenied => "permission_denied",
        ErrorCode::Cancelled => "cancelled",
        ErrorCode::Expired => "expired",
        ErrorCode::Unavailable | ErrorCode::Busy => "unavailable",
        _ => "failed",
    }
}
