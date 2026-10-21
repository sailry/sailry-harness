//! Durable row commands retain identity only while their outcome is unknown.
use super::*;
use sailry_protocol::RequestId;

pub(super) struct Submission {
    pub request: Request,
    pub revision: u64,
}

fn prepare(
    requests: &mut BTreeMap<connector::Id, Submission>,
    id: connector::Id,
    revision: u64,
    request: Request,
) -> Request {
    if matches!(&request.command, Command::ProbeConnection { .. }) {
        return request;
    }
    requests
        .entry(id)
        .or_insert(Submission { request, revision })
        .request
        .clone()
}

fn finish(
    requests: &mut BTreeMap<connector::Id, Submission>,
    id: connector::Id,
    request: RequestId,
    error: Option<ErrorCode>,
) {
    if requests.get(&id).is_some_and(|submission| {
        submission.request.id == request && error != Some(ErrorCode::OutcomeUnknown)
    }) {
        requests.remove(&id);
    }
}

pub(super) fn reconcile(
    requests: &mut BTreeMap<connector::Id, Submission>,
    pending: &BTreeSet<connector::Id>,
    rows: &[Connection],
) {
    requests.retain(|id, submission| {
        pending.contains(id)
            || !rows
                .iter()
                .any(|row| row.id == *id && row.revision > submission.revision)
    });
}

pub(super) fn observed_edit(request: &Request, rows: &[Connection]) -> Option<Connection> {
    let Command::UpdateConnection {
        connection,
        expected_revision,
        ..
    } = &request.command
    else {
        return None;
    };
    rows.iter()
        .find(|row| row.id == *connection && row.revision > *expected_revision)
        .cloned()
}

fn success_key(command: &Command) -> &'static str {
    match command {
        Command::StartConnectorLogin {
            method: connector::Kind::LocalCli,
            ..
        } => "connector_bound",
        Command::ProbeConnection { .. } => "connector_checked",
        Command::DisconnectConnection { .. } => "connector_disconnected",
        Command::RemoveConnection { .. } => "connector_removed",
        _ => "connector_saved",
    }
}

async fn execute(
    client: &sailry_client::Client,
    request: Request,
    stop: CancellationToken,
) -> Result<Output, Fault> {
    let output = client.execute(request).await?;
    let Output::ConnectorLogin(attempt) = output else {
        return Ok(output);
    };
    let observe = async {
        let interrupted = |_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "connector binding observation interrupted",
            )
        };
        let mut subscription = client
            .subscribe_connector_login(attempt.id)
            .await
            .map_err(interrupted)?;
        loop {
            let Update::ConnectorLogin(update) = subscription.next().await.map_err(interrupted)?
            else {
                return Err(Fault::invalid("connector login update expected"));
            };
            if update.attempt != attempt {
                return Err(Fault::invalid("connector login attempt changed"));
            }
            match update.state {
                connector::LoginState::Connected(connection) => {
                    return Ok(Output::Connection(connection));
                }
                connector::LoginState::Failed(error) => return Err(error),
                connector::LoginState::Cancelled => {
                    return Err(Fault::new(
                        ErrorCode::Cancelled,
                        "connector binding cancelled",
                    ));
                }
                _ => (),
            }
        }
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => {
            client.execute(client.prepare(Command::CancelConnectorLogin {
                attempt: attempt.id,
            })).await?;
            Err(Fault::new(ErrorCode::Cancelled, "connector binding cancelled"))
        }
        result = observe => result,
    }
}

impl Workspace {
    pub(super) fn refresh_connectors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(binding) = self.connectors.binding.clone() else {
            return;
        };
        if self.connectors.refreshing {
            return;
        }
        let mut requests = Vec::new();
        for row in &self.connectors.rows {
            if !super::menu::checkable(row) || self.connectors.pending.contains(&row.id) {
                continue;
            }
            let request = binding
                .client
                .prepare(Command::ProbeConnection { connection: row.id });
            requests.push((row.id, request));
        }
        if requests.is_empty() {
            self.connectors.close();
            self.warm_connectors(window, cx);
            return;
        }
        self.connectors.refreshing = true;
        self.connectors
            .pending
            .extend(requests.iter().map(|(id, _)| *id));
        let client = binding.client.clone();
        let expected = client.target();
        let pending = requests.iter().map(|(id, _)| *id).collect::<Vec<_>>();
        let job = binding.runtime.spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            for (id, request) in requests {
                let client = client.clone();
                tasks.spawn(async move { (id, request.id, client.execute(request).await) });
            }
            let mut results = Vec::new();
            while let Some(result) = tasks.join_next().await {
                results.push(result);
            }
            results
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this
                    .connectors
                    .binding
                    .as_ref()
                    .is_none_or(|binding| binding.client.target() != expected)
                {
                    return;
                }
                this.connectors.refreshing = false;
                for id in pending {
                    this.connectors.pending.remove(&id);
                }
                let mut fault = None;
                match result {
                    Ok(results) => {
                        for result in results {
                            match result {
                                Ok((id, request, result)) => {
                                    this.connectors.pending.remove(&id);
                                    finish(
                                        &mut this.connectors.requests,
                                        id,
                                        request,
                                        result.as_ref().err().map(|error| error.code),
                                    );
                                    if let Err(error) = result {
                                        fault.get_or_insert(error);
                                    }
                                }
                                Err(_) => {
                                    fault.get_or_insert_with(|| {
                                        Fault::new(
                                            ErrorCode::OutcomeUnknown,
                                            "connector refresh interrupted",
                                        )
                                    });
                                }
                            }
                        }
                    }
                    Err(_) => {
                        fault = Some(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "connector refresh interrupted",
                        ));
                    }
                }
                reconcile(
                    &mut this.connectors.requests,
                    &this.connectors.pending,
                    &this.connectors.rows,
                );
                if let Some(error) = fault {
                    failure(error, window, cx);
                } else {
                    crate::feedback::toast(
                        window,
                        tr("connector_checked"),
                        notification::Notification::success(tr("connector_checked")),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn connector_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(binding) = self.connectors.binding.clone() else {
            return;
        };
        let id = match &command {
            Command::StartConnectorLogin {
                connection: Some(connection),
                expected_revision: Some(_),
                method: connector::Kind::LocalCli,
                ..
            } => *connection,
            Command::ProbeConnection { connection }
            | Command::UpdateConnection { connection, .. }
            | Command::SetConnectionEnabled { connection, .. }
            | Command::DisconnectConnection { connection, .. }
            | Command::RemoveConnection { connection, .. }
            | Command::ConfirmConnectorConsent { connection, .. } => *connection,
            _ => return,
        };
        let success = success_key(&command);
        let probe = matches!(&command, Command::ProbeConnection { .. });
        if !probe
            && self.connectors.requests.get(&id).is_some_and(|submission| {
                serde_json::to_value(&submission.request.command).ok()
                    != serde_json::to_value(&command).ok()
            })
        {
            crate::feedback::error("", &tr("connector_unknown"), window, cx);
            return;
        }
        if !self.connectors.pending.insert(id) {
            return;
        }
        let client = binding.client.clone();
        let expected = client.target();
        let revision = match &command {
            Command::StartConnectorLogin {
                expected_revision: Some(expected_revision),
                ..
            } => *expected_revision,
            Command::UpdateConnection {
                expected_revision, ..
            }
            | Command::SetConnectionEnabled {
                expected_revision, ..
            }
            | Command::DisconnectConnection {
                expected_revision, ..
            }
            | Command::RemoveConnection {
                expected_revision, ..
            }
            | Command::ConfirmConnectorConsent {
                expected_revision, ..
            } => *expected_revision,
            _ => self
                .connectors
                .rows
                .iter()
                .find(|row| row.id == id)
                .map(|row| row.revision)
                .unwrap_or_default(),
        };
        let request = prepare(
            &mut self.connectors.requests,
            id,
            revision,
            client.prepare(command),
        );
        let request_id = request.id;
        let stop = self.connectors.stop.child_token();
        let job = binding
            .runtime
            .spawn(async move { execute(&client, request, stop).await });
        cx.notify();
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this
                    .connectors
                    .binding
                    .as_ref()
                    .is_none_or(|binding| binding.client.target() != expected)
                {
                    return;
                }
                this.connectors.pending.remove(&id);
                let code = match &result {
                    Ok(outcome) => outcome.as_ref().err().map(|error| error.code),
                    Err(_) => Some(ErrorCode::OutcomeUnknown),
                };
                finish(&mut this.connectors.requests, id, request_id, code);
                reconcile(
                    &mut this.connectors.requests,
                    &this.connectors.pending,
                    &this.connectors.rows,
                );
                match result {
                    Ok(Ok(_)) => crate::feedback::toast(
                        window,
                        tr(success),
                        notification::Notification::success(tr(success)),
                        cx,
                    ),
                    Ok(Err(error)) if error.code == ErrorCode::Cancelled => (),
                    Ok(Err(error)) => failure(error, window, cx),
                    _ => crate::feedback::error("", &tr("connector_unknown"), window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests;
