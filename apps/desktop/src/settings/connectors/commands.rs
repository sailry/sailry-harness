//! Row commands retain request identity until completion or a newer observed revision.
use super::*;

pub(super) struct Submission {
    pub request: Request,
    pub revision: u64,
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
            if !row.enabled
                || matches!(
                    row.state,
                    connector::State::Connecting | connector::State::NeedsConfirmation
                )
                || self.connectors.pending.contains(&row.id)
            {
                continue;
            }
            if self
                .connectors
                .requests
                .get(&row.id)
                .is_some_and(|submission| {
                    !matches!(&submission.request.command, Command::ProbeConnection { .. })
                })
            {
                crate::feedback::error("", &tr("connector_unknown"), window, cx);
                return;
            }
            let request = self
                .connectors
                .requests
                .entry(row.id)
                .or_insert_with(|| Submission {
                    request: binding
                        .client
                        .prepare(Command::ProbeConnection { connection: row.id }),
                    revision: row.revision,
                })
                .request
                .clone();
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
                tasks.spawn(async move { (id, client.execute(request).await) });
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
                if this.connectors.binding.as_ref().is_none_or(|binding| binding.client.target() != expected) { return; }
                this.connectors.refreshing = false;
                for id in pending { this.connectors.pending.remove(&id); }
                let mut fault = None;
                match result {
                    Ok(results) => for result in results {
                        match result {
                            Ok((id, result)) => {
                                this.connectors.pending.remove(&id);
                                if !matches!(&result, Err(error) if matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable)) {
                                    this.connectors.requests.remove(&id);
                                }
                                if let Err(error) = result { fault.get_or_insert(error); }
                            },
                            Err(_) => { fault.get_or_insert_with(|| Fault::new(ErrorCode::OutcomeUnknown, "connector refresh interrupted")); },
                        }
                    },
                    Err(_) => { fault = Some(Fault::new(ErrorCode::OutcomeUnknown, "connector refresh interrupted")); },
                }
                reconcile(&mut this.connectors.requests, &this.connectors.pending, &this.connectors.rows);
                if let Some(error) = fault { failure(error, window, cx); }
                else { crate::feedback::toast(window, tr("connector_saved"), notification::Notification::success(tr("connector_saved")), cx); }
                cx.notify();
            });
        }).detach();
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
            Command::ProbeConnection { connection }
            | Command::UpdateConnection { connection, .. }
            | Command::SetConnectionEnabled { connection, .. }
            | Command::DisconnectConnection { connection, .. }
            | Command::RemoveConnection { connection, .. }
            | Command::ConfirmConnectorConsent { connection, .. } => *connection,
            _ => return,
        };
        if self.connectors.requests.get(&id).is_some_and(|submission| {
            serde_json::to_value(&submission.request.command).ok()
                != serde_json::to_value(&command).ok()
        }) {
            crate::feedback::error("", &tr("connector_unknown"), window, cx);
            return;
        }
        if !self.connectors.pending.insert(id) {
            return;
        }
        let client = binding.client.clone();
        let expected = client.target();
        let revision = match &command {
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
        let request = self
            .connectors
            .requests
            .entry(id)
            .or_insert_with(|| Submission {
                request: client.prepare(command),
                revision,
            })
            .request
            .clone();
        let job = binding
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.notify();
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if this.connectors.binding.as_ref().is_none_or(|binding| binding.client.target() != expected) { return; }
                this.connectors.pending.remove(&id);
                if matches!(&result, Ok(outcome) if !matches!(outcome, Err(error) if matches!(error.code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable))) {
                    this.connectors.requests.remove(&id);
                }
                reconcile(&mut this.connectors.requests, &this.connectors.pending, &this.connectors.rows);
                match result {
                Ok(Ok(_)) => crate::feedback::toast(
                    window,
                    tr("connector_saved"),
                    notification::Notification::success(tr("connector_saved")),
                    cx,
                ),
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
