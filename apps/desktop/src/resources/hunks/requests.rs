use super::*;

impl Review {
    pub(super) fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.request.is_some() {
            return;
        }
        self.pending = true;
        let client = self.binding.client.clone();
        let session = self.session;
        let turn = self.turn;
        let path = self.path.clone();
        let job = self.binding.runtime.spawn(async move {
            let Output::TurnHunks(data) = client
                .execute(client.prepare(Command::ReadTurnHunks {
                    session,
                    turn,
                    path: path.clone(),
                }))
                .await?
            else {
                return Err(sailry_protocol::Fault::invalid(
                    "unexpected turn hunk response",
                ));
            };
            let (file, status) = tokio::join!(
                client.execute(client.prepare(Command::ReadFileLines {
                    worktree: data.worktree,
                    path,
                    offset: 0,
                    limit: 1,
                    expected_revision: None,
                })),
                client.execute(client.prepare(Command::InspectGit {
                    worktree: data.worktree
                })),
            );
            Ok((data, file?, status))
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |review, window, cx| {
                review.pending = false;
                match result {
                    Ok(Ok((data, Output::FileLines(file), status)))
                        if data.session == review.session
                            && data.turn == review.turn
                            && data.path == review.path
                            && file.path == review.path =>
                    {
                        review.binding.worktree = Some(data.worktree);
                        review.data = Some(data);
                        review.revision = Some(file.revision);
                        let status = match status {
                            Ok(Output::GitStatus(status)) => Some(status),
                            _ => None,
                        };
                        review.index = status
                            .as_ref()
                            .and_then(|status| status.index_revision.clone());
                        review.head = status.and_then(|status| status.head);
                        review.failed = false;
                    }
                    _ => {
                        review.failed = true;
                        review.report("hunks_load_failed", window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn apply(
        &mut self,
        id: &str,
        operation: hunk::Operation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || !self.connected(cx) {
            return;
        }
        let Some(data) = &self.data else { return };
        if !data.hunks.iter().any(|hunk| hunk.id == id) {
            return;
        }
        let Some(revision) = self.revision.clone() else {
            return;
        };
        let request = if let Some(request) = &self.request {
            request.clone()
        } else {
            let stage = operation == hunk::Operation::Stage;
            if stage && self.index.is_none() {
                return;
            }
            self.binding
                .client
                .prepare(Command::ApplyTurnHunk(hunk::Selection {
                    session: self.session,
                    turn: self.turn,
                    worktree: data.worktree,
                    path: self.path.clone(),
                    hunk: id.to_owned(),
                    operation,
                    expected_revision: revision,
                    expected_index: stage.then(|| self.index.clone()).flatten(),
                    expected_head: stage.then(|| self.head.clone()).flatten(),
                }))
        };
        let Command::ApplyTurnHunk(selection) = &request.command else {
            return;
        };
        let selected = selection.clone();
        let scope = (self.binding.client.target(), selected.worktree);
        if let Err(issue) =
            crate::plugins::file_transfers::recovery::reserve(scope, request.id, &selected.path, cx)
        {
            self.report(issue, window, cx);
            return;
        }
        let client = self.binding.client.clone();
        let request_id = request.id;
        self.request = Some(request.clone());
        self.pending = true;
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            let success = matches!(&result, Ok(Ok(Output::HunkApplied(value)))
                if value.session == selected.session && value.turn == selected.turn && value.path == selected.path && value.hunk == selected.hunk
                && (selected.operation == hunk::Operation::Stage) == value.index.is_some());
            let issue = if success { None } else { Some(match &result {
                Ok(Err(fault)) => match fault.code {
                    ErrorCode::RevisionConflict => "hunks_conflict",
                    ErrorCode::Busy => "hunks_busy",
                    ErrorCode::Unavailable | ErrorCode::OutcomeUnknown => "hunks_unknown",
                    _ => "hunks_failed",
                },
                _ => "hunks_unknown",
            }) };
            _ = cx.update(|window, cx| {
                crate::plugins::file_transfers::recovery::release(request_id, issue == Some("hunks_unknown"), cx);
                if success && selected.operation == hunk::Operation::Reject {
                    for documents in crate::plugins::file_transfers::documents(scope, cx) {
                        documents.update(cx, |documents, cx| documents.restored(&selected.path, false, window, cx));
                    }
                }
            });
            _ = owner.update_in(cx, |review, window, cx| {
                review.pending = false;
                if issue != Some("hunks_unknown") {
                    review.request = None;
                }
                if let Some(issue) = issue {
                    review.failed = true;
                    review.report(issue, window, cx);
                } else if let Ok(Ok(Output::HunkApplied(value))) = result {
                    review.failed = false;
                    review.revision = Some(value.revision);
                    if let Some(index) = value.index { review.index = Some(index.revision); }
                    review.decisions.insert(selected.hunk, if selected.operation == hunk::Operation::Stage { Decision::Staged } else { Decision::Rejected });
                    let key = if selected.operation == hunk::Operation::Stage { "hunks_staged" } else { "hunks_rejected" };
                    crate::feedback::toast(window, tr(key), Notification::success(tr(key)), cx);
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Request {
            command: Command::ApplyTurnHunk(selection),
            ..
        }) = &self.request
        {
            let id = selection.hunk.clone();
            let operation = selection.operation;
            self.apply(&id, operation, window, cx);
        } else {
            self.load(window, cx);
        }
    }
}
