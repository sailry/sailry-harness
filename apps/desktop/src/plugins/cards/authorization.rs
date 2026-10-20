//! Package-declared connectors own authorization through the shared Node service.
use super::*;
use host::LoginState;
use sailry_client::{connectors, oauth::Callback};
use sailry_protocol::{
    ErrorCode, Fault, Request, RequestOutcome,
    plugin::connector::{self, Attempt, LoginState as State},
};

pub(super) struct Login {
    pub attempt: Option<Attempt>,
    request: Option<Request>,
    pub(super) callback: Option<Callback>,
    pub url: Option<(String, u64)>,
    pub redirect: Option<String>,
    pub(super) state: LoginState,
    pending: bool,
    requested_browser: Option<(String, card::Mode)>,
    stop: CancellationToken,
    task: Option<Task<()>>,
    observer: Option<Task<()>>,
    receiver: Option<Task<()>>,
}

impl Default for Login {
    fn default() -> Self {
        Self {
            attempt: None,
            request: None,
            callback: None,
            url: None,
            redirect: None,
            state: LoginState::Idle,
            pending: false,
            requested_browser: None,
            stop: CancellationToken::new(),
            task: None,
            observer: None,
            receiver: None,
        }
    }
}

impl Login {
    pub(super) fn can_cancel(&self) -> bool {
        self.attempt.is_some() && self.request.is_none() && !self.pending
    }

    pub(super) fn can_open(&self) -> bool {
        matches!(self.state, LoginState::Pending)
            && self.redirect.is_some()
            && (self.callback.is_some() || self.receiver.is_some())
            && self.url.as_ref().is_some_and(|(_, expiry)| {
                *expiry > chrono::Utc::now().timestamp_millis().max(0) as u64
            })
    }

    pub(super) fn keeps_alive(&self) -> bool {
        self.pending || self.can_open()
    }

    pub(super) fn suspend(&mut self) {
        self.stop.cancel();
        self.task = None;
        self.observer = None;
        self.receiver = None;
        self.callback = None;
        self.url = None;
        self.redirect = None;
        self.pending = false;
        self.requested_browser = None;
        if self.request.is_some() {
            self.state = LoginState::Unconfirmed;
        }
    }
}

impl Card {
    pub(super) fn action(
        &mut self,
        host: Arc<host::Host>,
        action: host::Action,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active(cx)
            || self
                .mounted
                .as_ref()
                .is_none_or(|mounted| !Arc::ptr_eq(&mounted.host, &host))
        {
            self.toast(crate::tr("plugins_failed").to_string(), true, window, cx);
            return;
        }
        match action {
            host::Action::Connect { target, mode } => self.begin(target.zip(mode), window, cx),
            host::Action::Cancel => self.cancel(window, cx),
            host::Action::Refresh => host.refresh(id),
            host::Action::Settings => self.open_settings(host, window, cx),
            host::Action::Browser { target, mode } => self.open_browser(target, mode, window, cx),
        }
    }

    pub(super) fn login_state(&self, state: LoginState, can_open: bool) {
        let state = if self.authorization.request.is_some() && !self.authorization.pending {
            LoginState::Unconfirmed
        } else {
            state
        };
        if let Some(mounted) = &self.mounted {
            mounted.host.login(
                state,
                can_open && self.authorization.can_open(),
                self.authorization.can_cancel(),
            );
        }
    }

    fn begin(
        &mut self,
        browser: Option<(String, card::Mode)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.authorization.pending {
            return;
        }
        self.authorization.requested_browser = browser;
        if self.authorization.request.is_some() {
            self.send_login(true, window, cx);
            return;
        }
        if let Some(attempt) = self.authorization.attempt.clone() {
            if self.authorization.redirect.is_none() {
                self.authorization.requested_browser = None;
            }
            self.observe_login(attempt, window, cx);
            return;
        }
        let Some(host) = self.mounted.as_ref().map(|mounted| mounted.host.clone()) else {
            return;
        };
        let Some(server) = host.declaration.mcp_server.clone() else {
            return;
        };
        let callback = match Callback::bind("/auth/callback") {
            Ok(callback) => callback,
            Err(_) => {
                self.toast(
                    crate::tr("mcp_oauth_callback_failed").to_string(),
                    true,
                    window,
                    cx,
                );
                return;
            }
        };
        self.authorization.stop.cancel();
        self.authorization.stop = CancellationToken::new();
        self.authorization.redirect = Some(callback.redirect.clone());
        let redirect = callback.redirect.clone();
        self.authorization.callback = Some(callback);
        self.authorization.state = LoginState::Starting;
        self.authorization.pending = true;
        self.login_state(LoginState::Starting, false);
        let stop = self.authorization.stop.clone();
        let job=self.binding.runtime.spawn(async move {
            tokio::select! { _=stop.cancelled()=>None,result=prepare(&host,&server,redirect)=>Some(result) }
        });
        self.authorization.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                card.authorization.pending = false;
                if !card.active(cx) {
                    return;
                }
                match result {
                    Ok(Some(Ok(request))) => {
                        card.authorization.request = Some(request);
                        card.send_login(false, window, cx);
                    }
                    Ok(Some(Err(_))) => {
                        card.authorization.suspend();
                        card.authorization.state = LoginState::Failed;
                        card.login_state(LoginState::Failed, false);
                        card.toast(crate::tr("mcp_oauth_failed").to_string(), true, window, cx);
                    }
                    _ => {}
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.authorization.pending {
            return;
        }
        if self.authorization.request.is_some() {
            self.send_login(true, window, cx);
            return;
        }
        let Some(attempt) = &self.authorization.attempt else {
            return;
        };
        let Some(host) = self.mounted.as_ref().map(|mounted| mounted.host.clone()) else {
            return;
        };
        let Ok(request) = host.request(Command::CancelConnectorLogin {
            attempt: attempt.id,
        }) else {
            return;
        };
        self.authorization.request = Some(request);
        self.send_login(false, window, cx);
    }

    fn send_login(&mut self, inspect: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.authorization.pending || !self.active(cx) {
            return;
        }
        let Some(request) = self.authorization.request.clone() else {
            return;
        };
        self.authorization.pending = true;
        let command = request.command.clone();
        let client = self.binding.client.clone();
        let job = self.binding.runtime.spawn(async move {
            if inspect {
                inspected(client.outcome(&request).await)
            } else {
                let result = client.execute(request).await;
                let confirmed = !result.as_ref().is_err_and(|error| uncertain(error.code));
                (result, confirmed)
            }
        });
        self.authorization.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                card.authorization.pending = false;
                if !card.active(cx) {
                    return;
                }
                match result {
                    Ok((Ok(Output::ConnectorLogin(attempt)), _)) => {
                        card.authorization.request = None;
                        match command {
                            Command::StartConnectorLogin { .. } => {
                                card.observe_login(attempt, window, cx)
                            }
                            Command::CancelConnectorLogin { .. } => {
                                card.authorization.suspend();
                                card.authorization.attempt = None;
                                card.authorization.state = LoginState::Cancelled;
                                card.login_state(LoginState::Cancelled, false);
                            }
                            Command::SubmitConnectorInput { .. } => {}
                            _ => {}
                        }
                    }
                    Ok((Err(_), true)) => {
                        card.authorization.request = None;
                        card.authorization.url = None;
                        card.authorization.requested_browser = None;
                        card.authorization.state = LoginState::Failed;
                        card.login_state(LoginState::Failed, false);
                        card.toast(crate::tr("mcp_oauth_failed").to_string(), true, window, cx);
                    }
                    _ => {
                        card.authorization.state = LoginState::Unconfirmed;
                        card.login_state(LoginState::Unconfirmed, false);
                        card.toast(crate::tr("plugins_unknown").to_string(), true, window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn observe_login(
        &mut self,
        attempt: Attempt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.authorization.observer = None;
        self.authorization.attempt = Some(attempt.clone());
        if self.authorization.stop.is_cancelled() {
            self.authorization.stop = CancellationToken::new();
        }
        let (sender, mut receiver) = tokio::sync::watch::channel(connectors::View::default());
        let client = self.binding.client.clone();
        let stop = self.authorization.stop.clone();
        self.binding.runtime.spawn(async move {
            let _ = client.watch_connector_login(attempt, sender, stop).await;
        });
        self.authorization.observer = Some(cx.spawn_in(window, async move |owner, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if owner
                    .update_in(cx, |card, window, cx| {
                        if !card.active(cx) {
                            card.authorization.suspend();
                            return;
                        }
                        if let Some(update) = &view.update {
                            match &update.state {
                                State::Starting => card.login_state(LoginState::Starting, false),
                                State::Browser { url, expires_at_ms } if view.connected => {
                                    card.authorization.url = Some((url.clone(), *expires_at_ms));
                                    card.authorization.state = LoginState::Pending;
                                    card.login_state(
                                        LoginState::Pending,
                                        card.authorization.redirect.is_some(),
                                    );
                                    card.receive_callback(url.clone(), window, cx);
                                    if let Some((target, mode)) =
                                        card.authorization.requested_browser.take()
                                    {
                                        card.open_browser(target, mode, window, cx);
                                    }
                                }
                                State::Exchanging => {
                                    card.authorization.url = None;
                                    card.authorization.state = LoginState::Exchanging;
                                    card.login_state(LoginState::Exchanging, false);
                                }
                                State::Connected(reference) => {
                                    let notify =
                                        !matches!(card.authorization.state, LoginState::Connected);
                                    card.authorization.suspend();
                                    card.authorization.attempt = None;
                                    card.authorization.request = None;
                                    card.authorization.state = LoginState::Connected;
                                    if let Some(mounted) = &card.mounted {
                                        mounted.host.reference(reference.package.clone());
                                    }
                                    card.login_state(LoginState::Connected, false);
                                    if notify {
                                        crate::ui::authorization::connected(
                                            &card.summary.package.name,
                                            reference
                                                .identity
                                                .as_ref()
                                                .map(|identity| identity.name.as_str()),
                                            window,
                                            cx,
                                        );
                                    }
                                }
                                State::Cancelled => {
                                    card.authorization.suspend();
                                    card.authorization.attempt = None;
                                    card.authorization.state = LoginState::Cancelled;
                                    card.login_state(LoginState::Cancelled, false);
                                }
                                State::Failed(_) => {
                                    card.authorization.suspend();
                                    card.authorization.attempt = None;
                                    card.authorization.state = LoginState::Failed;
                                    card.login_state(LoginState::Failed, false);
                                    card.toast(
                                        crate::tr("mcp_oauth_failed").to_string(),
                                        true,
                                        window,
                                        cx,
                                    );
                                }
                                _ => {
                                    card.authorization.url = None;
                                    card.login_state(LoginState::Pending, false);
                                }
                            }
                        }
                        if view.error.is_some() {
                            card.authorization.url = None;
                            card.authorization.state = LoginState::Unconfirmed;
                            card.authorization.requested_browser = None;
                            card.login_state(LoginState::Unconfirmed, false);
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

    fn receive_callback(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(callback) = self.authorization.callback.take() else {
            return;
        };
        let stop = self.authorization.stop.clone();
        let message = crate::tr("mcp_oauth_return").to_string();
        let job = self.binding.runtime.spawn(callback.receive(
            url,
            async move { sailry_client::oauth::Reply::Text(message) },
            stop,
        ));
        self.authorization.receiver = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |card, window, cx| {
                if !card.active(cx) {
                    return;
                }
                card.authorization.receiver = None;
                card.authorization.redirect = None;
                card.authorization.url = None;
                let Some(attempt) = &card.authorization.attempt else {
                    return;
                };
                match result {
                    Ok(Ok(callback)) => {
                        let Some(host) = card.mounted.as_ref().map(|mounted| mounted.host.clone())
                        else {
                            return;
                        };
                        if let Ok(request) = host.request(Command::SubmitConnectorInput {
                            attempt: attempt.id,
                            input: callback,
                        }) {
                            card.authorization.request = Some(request);
                            card.login_state(card.authorization.state.clone(), false);
                            card.send_login(false, window, cx);
                        }
                    }
                    _ => {
                        card.authorization.state = LoginState::Unconfirmed;
                        card.login_state(LoginState::Unconfirmed, false);
                        card.toast(
                            crate::tr("mcp_oauth_callback_failed").to_string(),
                            true,
                            window,
                            cx,
                        );
                    }
                }
            });
        }));
    }
}

pub(super) fn inspected(outcome: Result<RequestOutcome, Fault>) -> (Result<Output, Fault>, bool) {
    match outcome {
        Ok(RequestOutcome::Completed(result)) => (*result, true),
        Err(error) => (Err(error), false),
        _ => (
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "login outcome is unknown",
            )),
            false,
        ),
    }
}

pub(super) fn uncertain(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::OutcomeUnknown
            | ErrorCode::Unavailable
            | ErrorCode::Busy
            | ErrorCode::Internal
            | ErrorCode::Cancelled
    )
}

async fn prepare(host: &host::Host, server: &str, redirect: String) -> Result<Request, Fault> {
    let package = host
        .package()
        .map_err(|error| Fault::new(ErrorCode::Unavailable, error.to_string()))?;
    let client = &host.binding.client;
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::ReadPlugin {
            name: package.name.clone(),
        }))
        .await?
    else {
        return Err(Fault::invalid("plugin metadata expected"));
    };
    if info.summary.reference() != package || !info.summary.enabled {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin configuration changed",
        ));
    }
    let id = connector_id(&info, server)?;
    let request = host
        .request(Command::ListConnections)
        .map_err(|error| Fault::new(ErrorCode::Unavailable, error.to_string()))?;
    let Output::Connections(connections) = client.execute(request).await? else {
        return Err(Fault::invalid("connections expected"));
    };
    let connection = connections
        .into_iter()
        .find(|connection| connection.package.name == package.name && connection.connector == id);
    host.request(Command::StartConnectorLogin {
        package,
        connector: id,
        connection: connection.as_ref().map(|connection| connection.id),
        expected_revision: connection.as_ref().map(|connection| connection.revision),
        method: connector::Kind::McpOauth,
        input: connector::Input {
            redirect: Some(redirect),
            ..Default::default()
        },
    })
    .map_err(|error| Fault::new(ErrorCode::Unavailable, error.to_string()))
}

pub(super) fn connector_id(info: &plugin::Info, server: &str) -> Result<String, Fault> {
    let mut matches = info
        .extension
        .iter()
        .flat_map(|extension| &extension.connectors)
        .filter(|declaration| {
            declaration.auth.iter().any(
                |auth| matches!(auth,connector::Auth::McpOauth { server:name,.. } if name==server),
            )
        });
    let declaration = matches
        .next()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "card connector is unavailable"))?;
    if matches.next().is_some() {
        return Err(Fault::invalid("card connector is ambiguous"));
    }
    Ok(declaration.id.clone())
}
