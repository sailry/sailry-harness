use super::*;
use sailry_protocol::{
    TurnId,
    conversation::{Status, question},
};
use std::{num::NonZeroU64, sync::atomic::Ordering};

impl Adapter {
    pub(super) async fn prompt(
        self: &Arc<Self>,
        prompt: acp::PromptRequest,
        connection: &ConnectionTo<sdk::Client>,
        cancelled: sdk::RequestCancellation,
    ) -> acp::Result<acp::PromptResponse> {
        let opened = self.opened(&prompt.session_id).await?;
        let session = opened.session.lock().await.clone();
        let revision = member(prompt.meta.as_ref(), "expected_revision")?
            .map(|value| {
                value
                    .as_u64()
                    .ok_or_else(|| invalid("expected_revision must be an integer"))
            })
            .transpose()?
            .unwrap_or(session.revision);
        let message = content(&prompt.prompt)?;
        let request = self.prepare(
            Command::SubmitTurn {
                session: session.id,
                expected_revision: revision,
                message: message.into(),
            },
            prompt.meta.as_ref(),
        )?;
        let mut context = json!({"node":self.client.target(),"session":session.id,"request_id":request.id,"revision":revision,"state":"admitting"});
        let update = acp::SessionUpdate::SessionInfoUpdate(acp::SessionInfoUpdate::new());
        notify(
            connection,
            &prompt.session_id,
            update.clone(),
            context.clone(),
        )?;
        // Admission is allowed to finish before observing RPC cancellation. The Node owns it.
        let output = self
            .client
            .execute(request)
            .await
            .map_err(|error| outcome(error, &context))?;
        let Output::QueuedTurn(turn) = output else {
            return Err(outcome(
                Fault::invalid("turn admission response expected"),
                &context,
            ));
        };
        context["turn"] = json!(turn.id);
        context["revision"] = json!(turn.revision);
        context["state"] = json!("admitted");
        notify(connection, &prompt.session_id, update, context.clone())?;
        let run = self
            .locate(session.id, turn.id)
            .await
            .map_err(|error| outcome(error, &context))?;
        context["revision"] = json!(run.revision);
        if run.sequence < opened.history.fetch_min(run.sequence, Ordering::SeqCst) {
            let sequence = NonZeroU64::new(run.sequence).ok_or_else(|| {
                outcome(
                    Fault::invalid("turn admission sequence is missing"),
                    &context,
                )
            })?;
            opened
                .requests
                .send(HistoryRequest::Through(sequence))
                .await
                .map_err(|_| {
                    outcome(
                        Fault::new(
                            ErrorCode::Unavailable,
                            "conversation history request closed",
                        ),
                        &context,
                    )
                })?;
        }
        let mut receiver = opened.view.clone();
        let mut problem = opened.problem.subscribe();
        loop {
            let view = receiver.borrow_and_update().clone();
            let observed_problem = problem.borrow_and_update().clone();
            if let Some(problem) = observed_problem
                && relevant(&problem, session.id, turn.id, &view)
            {
                if problem.operation["state"] == "awaiting_input" {
                    context["state"] = json!("awaiting_input");
                    context["operation"] = problem.operation;
                    return Err(sdk::Error::new(-32000, problem.error.message)
                        .data(json!({"sailry":context})));
                }
                context["operation"] = problem.operation;
                return Err(outcome(problem.error, &context));
            }
            if let Some(error) = view.older_error.as_ref().or(view.error.as_ref())
                && !matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy)
            {
                return Err(outcome(error.clone(), &context));
            }
            if view.connected
                && let Some(snapshot) = &view.snapshot
            {
                let page = &snapshot.page;
                if let Some(question) = page.questions.iter().find(|question| {
                    question.turn == turn.id && question.state == question::State::Pending
                }) {
                    self.flush(&opened, connection, &context, &cancelled)
                        .await?;
                    context["state"] = json!("awaiting_input");
                    context["question"] = json!(question);
                    return Err(
                        sdk::Error::new(-32000, "Respond to this question in Sailry")
                            .data(json!({"sailry":context})),
                    );
                }
                if let Some(run) = page.runs.iter().find(|run| run.turn == turn.id) {
                    context["revision"] = json!(run.revision);
                    context["state"] = json!(run.status);
                    if run.status == Status::Queued
                        && page.queue.paused
                        && page.queue.items.iter().any(|item| item.turn == turn.id)
                    {
                        context["state"] = json!("paused");
                        return Err(sdk::Error::new(
                            -32000,
                            "The admitted turn is paused in Sailry",
                        )
                        .data(json!({"sailry":context})));
                    }
                    let stop = match run.status {
                        Status::Queued | Status::Running | Status::Stopping => None,
                        Status::Completed
                            if page.children.iter().any(|child| {
                                child.origin.turn == turn.id
                                    && matches!(
                                        child.run.status,
                                        Status::Queued | Status::Running | Status::Stopping
                                    )
                            }) =>
                        {
                            None
                        }
                        Status::Completed => Some(acp::StopReason::EndTurn),
                        Status::Cancelled | Status::Interrupted => Some(acp::StopReason::Cancelled),
                        Status::Continued => {
                            self.flush(&opened, connection, &context, &cancelled)
                                .await?;
                            return Err(sdk::Error::new(
                                -32000,
                                "This turn handed off to another admitted turn",
                            )
                            .data(json!({"sailry":context})));
                        }
                        Status::Failed => {
                            self.flush(&opened, connection, &context, &cancelled)
                                .await?;
                            return Err(outcome(
                                run.error.clone().unwrap_or_else(|| {
                                    Fault::new(ErrorCode::Internal, "Agent execution failed")
                                }),
                                &context,
                            ));
                        }
                    };
                    if let Some(stop) = stop {
                        self.flush(&opened, connection, &context, &cancelled)
                            .await?;
                        return Ok(acp::PromptResponse::new(stop).meta(metadata(context)));
                    }
                }
            }
            tokio::select! {
                _ = opened.stop.cancelled() => return Err(observation(&context)),
                _ = cancelled.cancelled() => return Err(observation(&context)),
                changed = receiver.changed() => if changed.is_err() { return Err(outcome(Fault::new(ErrorCode::Unavailable, "conversation observation closed"), &context)); },
                changed = problem.changed() => if changed.is_err() { return Err(observation(&context)); },
            }
        }
    }

    async fn flush(
        self: &Arc<Self>,
        opened: &Arc<Opened>,
        connection: &ConnectionTo<sdk::Client>,
        context: &Value,
        cancelled: &sdk::RequestCancellation,
    ) -> acp::Result<()> {
        tokio::select! {
            _ = opened.stop.cancelled() => Err(observation(context)),
            _ = cancelled.cancelled() => Err(observation(context)),
            result = observe::flush(self, opened, connection) => result.map_err(|error| captured(error, context.clone())),
        }
    }

    async fn locate(
        &self,
        session: SessionId,
        turn: TurnId,
    ) -> std::result::Result<sailry_protocol::conversation::Run, Fault> {
        let mut before = None;
        loop {
            let history = self.client.read_conversation(session, before, 20).await?;
            if let Some(run) = history.page.runs.iter().find(|run| run.turn == turn) {
                return Ok(run.clone());
            }
            match history.page.next_before {
                Some(cursor) if Some(cursor) != before => before = Some(cursor),
                _ => {
                    return Err(Fault::new(
                        ErrorCode::NotFound,
                        "admitted turn is not in this conversation",
                    ));
                }
            }
        }
    }

    pub(super) async fn cancel(&self, notification: acp::CancelNotification) -> acp::Result<()> {
        let opened = self.opened(&notification.session_id).await?;
        let session = opened.session.lock().await.id;
        let history = self
            .client
            .read_conversation(session, None, 100)
            .await
            .map_err(|error| fault(error, None))?;
        let selected = member(notification.meta.as_ref(), "turn")?
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| invalid("turn must be a string"))?
                    .parse::<TurnId>()
                    .map_err(|_| invalid("turn identifier is invalid"))
            })
            .transpose()?;
        let run = history
            .page
            .runs
            .iter()
            .filter(|run| selected.is_none_or(|selected| run.turn == selected))
            .filter(|run| {
                matches!(
                    run.status,
                    Status::Running | Status::Stopping | Status::Queued
                )
            })
            .min_by_key(|run| (run.status == Status::Queued, run.sequence));
        if let Some(run) = run {
            self.execute(
                Command::StopTurn { turn: run.turn },
                notification.meta.as_ref(),
            )
            .await?;
        }
        Ok(())
    }
}

fn relevant(problem: &Problem, session: SessionId, turn: TurnId, view: &View) -> bool {
    if problem.operation.get("approval").is_none() {
        return true;
    }
    if !problem.operation["root_turn"].is_null() {
        return problem.operation["root_turn"] == json!(turn);
    }
    if problem.operation["session"] == json!(session) {
        return problem.operation["turn"] == json!(turn);
    }
    view.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.page.children.iter().any(|child| {
            child.origin.turn == turn
                && problem.operation["session"] == json!(child.run.session)
                && problem.operation["turn"] == json!(child.run.turn)
        })
    })
}

fn content(blocks: &[acp::ContentBlock]) -> acp::Result<String> {
    let mut message = String::new();
    for block in blocks {
        let text = match block {
            acp::ContentBlock::Text(text) => text.text.clone(),
            acp::ContentBlock::ResourceLink(resource) => format!(
                "{}: {}",
                resource.title.as_ref().unwrap_or(&resource.name),
                resource.uri
            ),
            _ => {
                return Err(invalid(
                    "this Node adapter accepts text and resource links; embedded media is unavailable",
                ));
            }
        };
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&text);
    }
    if message.trim().is_empty() {
        return Err(invalid("prompt content is empty"));
    }
    Ok(message)
}

fn outcome(error: Fault, context: &Value) -> sdk::Error {
    let mut context = context.clone();
    if !matches!(context["state"].as_str(), Some("failed")) {
        context["state"] = json!(if error.code == ErrorCode::OutcomeUnknown {
            "uncertain"
        } else if context["state"] == "admitting" {
            "rejected"
        } else {
            "observation_failed"
        });
    }
    context["error"] = json!(error);
    sdk::Error::new(-32000, error.message).data(json!({"sailry":context}))
}

fn observation(context: &Value) -> sdk::Error {
    let mut context = context.clone();
    context["state"] = json!("observation_cancelled");
    sdk::Error::new(
        -32800,
        "ACP observation cancelled; the admitted turn remains in Sailry",
    )
    .data(json!({"sailry":context}))
}
