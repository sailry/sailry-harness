use super::*;
use sailry_client::conversation::tools::{Call, State};
use sailry_protocol::{
    ApprovalId, TurnId,
    conversation::{Approval, ApprovalState, Part, Status},
};
use std::{collections::BTreeSet, future::Future, num::NonZeroU64, pin::Pin};

#[derive(Default)]
pub(super) struct Presentation {
    text: BTreeMap<(SessionId, String, usize), String>,
    calls: BTreeMap<String, acp::ToolCall>,
    permissions: BTreeSet<ApprovalId>,
    children: BTreeSet<SessionId>,
    views: BTreeMap<SessionId, watch::Receiver<View>>,
}

pub(super) async fn start(
    adapter: Arc<Adapter>,
    opened: Arc<Opened>,
    resumed: bool,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let session = opened.session.lock().await.id;
    render(
        &adapter,
        &opened,
        session,
        &opened.view,
        resumed,
        connection,
    )
    .await?;
    pump(
        adapter,
        opened.clone(),
        session,
        opened.view.clone(),
        connection,
    )?;
    Ok(())
}

fn pump(
    adapter: Arc<Adapter>,
    opened: Arc<Opened>,
    source: SessionId,
    mut receiver: watch::Receiver<View>,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let connected = connection.clone();
    connection.spawn(async move {
        loop {
            drop(receiver.borrow_and_update());
            if let Err(error) =
                render(&adapter, &opened, source, &receiver, false, &connected).await
            {
                opened.problem.send_replace(Some(Problem {
                    error: Fault::new(ErrorCode::Unavailable, error.to_string()),
                    operation: json!({"session":source,"state":"observation_failed"}),
                }));
                return Ok(());
            }
            tokio::select! {
                _ = opened.stop.cancelled() => return Ok(()),
                changed = receiver.changed() => if changed.is_err() { return Ok(()); },
            }
        }
    })
}

pub(super) async fn flush(
    adapter: &Arc<Adapter>,
    opened: &Arc<Opened>,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let source = opened.session.lock().await.id;
    render(adapter, opened, source, &opened.view, false, connection).await?;
    let terminal = opened
        .view
        .borrow()
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.page.children.clone())
        .unwrap_or_default();
    let children = opened.presentation.lock().await.views.clone();
    for (source, mut view) in children {
        for child in terminal.iter().filter(|child| {
            child.run.session == source
                && !matches!(
                    child.run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
        }) {
            loop {
                let current = view.borrow_and_update().clone();
                if let Some(error) = &current.older_error {
                    return Err(fault(error.clone(), None));
                }
                if current.connected
                    && current.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot
                            .page
                            .runs
                            .iter()
                            .any(|run| run.turn == child.run.turn && run.status == child.run.status)
                    })
                {
                    break;
                }
                tokio::select! {
                    _ = opened.stop.cancelled() => return Err(fault(Fault::new(ErrorCode::Cancelled, "child observation closed"), None)),
                    changed = view.changed() => if changed.is_err() { return Err(fault(Fault::new(ErrorCode::Unavailable, "child observation closed"), None)); },
                }
            }
        }
        render(adapter, opened, source, &view, false, connection).await?;
    }
    Ok(())
}

fn render<'a>(
    adapter: &'a Arc<Adapter>,
    opened: &'a Arc<Opened>,
    source: SessionId,
    receiver: &'a watch::Receiver<View>,
    seed: bool,
    connection: &'a ConnectionTo<sdk::Client>,
) -> Pin<Box<dyn Future<Output = acp::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let parent = opened.session.lock().await.id;
        let view = receiver.borrow().clone();
        let Some(snapshot) = &view.snapshot else {
            return Ok(());
        };
        let session = acp::SessionId::new(parent.to_string());
        let page = &snapshot.page;
        let mut calls = Vec::with_capacity(view.calls.len());
        for call in view.calls.iter() {
            let detail = adapter
                .client
                .tool_detail(
                    call.detail_reference(page)
                        .ok_or_else(|| invalid("tool detail identity is unavailable"))?,
                )
                .await
                .map_err(|error| fault(error, None))?;
            calls.push(call.with_detail(detail));
        }
        let mut presentation = opened.presentation.lock().await;
        if source != parent {
            if let Some(question) = page.questions.iter().find(|question| {
                question.state == sailry_protocol::conversation::question::State::Pending
            }) {
                opened.problem.send_replace(Some(Problem { error: Fault::new(ErrorCode::Busy, "Respond to the delegated question in Sailry"),
                operation: json!({"node":adapter.client.target(),"session":source,"turn":question.turn,"question":question,"state":"awaiting_input"}) }));
            } else {
                let clear = opened.problem.borrow().as_ref().is_some_and(|problem| {
                    problem.operation["state"] == "awaiting_input"
                        && problem.operation["session"] == json!(source)
                });
                if clear {
                    opened.problem.send_replace(None);
                }
            }
        }
        for entry in &page.entries {
            emit_parts(
                &mut presentation,
                Chunk {
                    source,
                    entry: &entry.id,
                    turn: entry.turn,
                    author: &entry.author,
                    parts: &entry.parts,
                },
                seed,
                &session,
                connection,
            )?;
        }
        for draft in &snapshot.drafts {
            emit_parts(
                &mut presentation,
                Chunk {
                    source,
                    entry: &draft.id,
                    turn: draft.turn,
                    author: &draft.author,
                    parts: &draft.parts,
                },
                seed,
                &session,
                connection,
            )?;
        }
        let mut permissions = Vec::new();
        for call in &calls {
            let id = call_id(source, call);
            let title = title(call, page);
            let context = json!({"node":adapter.client.target(),"session":source,"turn":call.turn,"source":call.source,
            "state":call.state,"approval":call.approval,"question":call.question});
            let tool = acp::ToolCall::new(id.clone(), title)
                .status(status(call, page))
                .raw_input(call.arguments(page).cloned())
                .raw_output(call.result(page).cloned())
                .content(
                    call.result(page)
                        .map(|result| {
                            vec![acp::ToolCallContent::from(acp::ContentBlock::Text(
                                acp::TextContent::new(
                                    result
                                        .as_str()
                                        .map_or_else(|| result.to_string(), str::to_owned),
                                ),
                            ))]
                        })
                        .unwrap_or_default(),
                )
                .meta(metadata(context.clone()));
            if let Some(previous) = presentation.calls.insert(id.clone(), tool.clone()) {
                if previous != tool && !seed {
                    notify(
                        connection,
                        &session,
                        acp::SessionUpdate::ToolCallUpdate(tool.into()),
                        context,
                    )?;
                }
            } else if !seed {
                notify(
                    connection,
                    &session,
                    acp::SessionUpdate::ToolCall(tool),
                    context,
                )?;
            }
            if let Some(approval) = &call.approval
                && approval.state == ApprovalState::Pending
                && presentation.permissions.insert(approval.id)
            {
                permissions.push((call.clone(), approval.clone()));
            }
        }
        let children = page
            .children
            .iter()
            .filter(|child| {
                matches!(
                    child.run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
            })
            .filter(|child| presentation.children.insert(child.run.session))
            .map(|child| child.run.clone())
            .collect::<Vec<_>>();
        drop(presentation);
        for (call, approval) in permissions {
            permission(
                adapter.clone(),
                opened.clone(),
                source,
                call,
                approval,
                connection,
            )?;
        }
        for child in children {
            let sequence = NonZeroU64::new(child.sequence)
                .ok_or_else(|| invalid("child admission sequence is missing"))?;
            let stop = opened.stop.child_token();
            let (updates, receiver) = watch::channel(View::default());
            let (requests, requested) = mpsc::channel(1);
            requests
                .try_send(HistoryRequest::Through(sequence))
                .map_err(|_| invalid("child history request is unavailable"))?;
            let client = adapter.client.clone();
            let child_stop = stop.clone();
            let child_id = child.session;
            let watching = tokio::spawn(async move {
                client
                    .watch_conversation(child_id, updates, child_stop, requested)
                    .await
            });
            opened
                .presentation
                .lock()
                .await
                .views
                .insert(child_id, receiver.clone());
            let linked = opened.clone();
            connection.spawn(async move {
                let _requests = requests;
                linked.stop.cancelled().await;
                stop.cancel();
                watching.abort();
                Ok(())
            })?;
            pump(
                adapter.clone(),
                opened.clone(),
                child_id,
                receiver,
                connection,
            )?;
        }
        Ok(())
    })
}

struct Chunk<'a> {
    source: SessionId,
    entry: &'a str,
    turn: TurnId,
    author: &'a str,
    parts: &'a [Part],
}

fn emit_parts(
    presentation: &mut Presentation,
    chunk: Chunk<'_>,
    seed: bool,
    session: &acp::SessionId,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let Chunk {
        source,
        entry,
        turn,
        author,
        parts,
    } = chunk;
    for (index, part) in parts.iter().enumerate() {
        let (value, thought) = match part {
            Part::Text(value) => (value, false),
            Part::Thinking(value) => (value, true),
            _ => continue,
        };
        let previous = presentation
            .text
            .entry((source, entry.into(), index))
            .or_default();
        if seed {
            previous.clone_from(value);
            continue;
        }
        let delta = value.strip_prefix(previous.as_str()).ok_or_else(|| {
            fault(
                Fault::new(
                    ErrorCode::Conflict,
                    "conversation text changed; reload the ACP session",
                ),
                None,
            )
        })?;
        if delta.is_empty() {
            continue;
        }
        let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(delta)))
            .message_id(acp::MessageId::new(format!("{source}:{entry}")))
            .meta(metadata(
                json!({"session":source,"turn":turn,"entry":entry,"part":index}),
            ));
        let update = if author == "user" {
            acp::SessionUpdate::UserMessageChunk(chunk)
        } else if thought {
            acp::SessionUpdate::AgentThoughtChunk(chunk)
        } else {
            acp::SessionUpdate::AgentMessageChunk(chunk)
        };
        notify(
            connection,
            session,
            update,
            json!({"session":source,"turn":turn,"entry":entry,"part":index}),
        )?;
        previous.clone_from(value);
    }
    Ok(())
}

fn call_id(session: SessionId, call: &Call) -> String {
    format!("{session}:{}", call.source.key())
}

fn title(call: &Call, page: &sailry_protocol::conversation::Page) -> String {
    let title = call
        .arguments(page)
        .and_then(|arguments| arguments.get("description"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&call.name);
    if call.approval.as_ref().is_some_and(|approval| {
        approval.access == Some(sailry_protocol::process::Access::Unrestricted)
    }) {
        format!("{title} — allow filesystem access outside the workspace and network access")
    } else {
        title.into()
    }
}

fn status(call: &Call, page: &sailry_protocol::conversation::Page) -> acp::ToolCallStatus {
    match call.state {
        State::Waiting => acp::ToolCallStatus::Pending,
        State::Running => acp::ToolCallStatus::InProgress,
        State::Returned
            if call
                .result(page)
                .is_some_and(|value| value.get("error").is_some()) =>
        {
            acp::ToolCallStatus::Failed
        }
        State::Returned => acp::ToolCallStatus::Completed,
        State::Cancelled | State::NotExecuted | State::Interrupted => acp::ToolCallStatus::Failed,
    }
}

struct Permission {
    source: SessionId,
    call: Call,
    approval: Approval,
    decision: RequestId,
}

fn permission(
    adapter: Arc<Adapter>,
    opened: Arc<Opened>,
    source: SessionId,
    call: Call,
    approval: Approval,
    connection: &ConnectionTo<sdk::Client>,
) -> acp::Result<()> {
    let connected = connection.clone();
    connection.spawn(async move {
        let parent = opened.session.lock().await.id;
        let root_turn = if source == parent {
            Some(approval.turn)
        } else {
            opened.view.borrow().snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .page
                    .children
                    .iter()
                    .find(|child| child.run.session == source && child.run.turn == approval.turn)
                    .map(|child| child.origin.turn)
            })
        };
        let permission = Permission {
            source,
            call,
            approval,
            decision: RequestId::new(),
        };
        let mut operation = json!({
            "node":adapter.client.target(), "session":source, "turn":permission.approval.turn,
            "root_turn":root_turn, "approval":permission.approval.id,
            "decision_request_id":permission.decision, "state":"observation_failed"
        });
        if let Err(error) = decide(&adapter, &opened, &permission, &connected, &mut operation).await
        {
            let resolving = operation["state"] == "resolving";
            let code = if resolving {
                error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("sailry"))
                    .and_then(|data| data.get("code"))
                    .and_then(|code| serde_json::from_value(code.clone()).ok())
                    .unwrap_or(ErrorCode::Unavailable)
            } else {
                ErrorCode::Unavailable
            };
            operation["state"] = json!(if code == ErrorCode::OutcomeUnknown {
                "uncertain"
            } else if resolving {
                "rejected"
            } else {
                "observation_failed"
            });
            let message = error.message.clone();
            operation["error"] = json!(error);
            opened.problem.send_replace(Some(Problem {
                error: Fault::new(code, message),
                operation,
            }));
        }
        Ok(())
    })
}

async fn decide(
    adapter: &Adapter,
    opened: &Opened,
    permission: &Permission,
    connection: &ConnectionTo<sdk::Client>,
    operation: &mut Value,
) -> acp::Result<()> {
    let Permission {
        source,
        call,
        approval,
        decision,
    } = permission;
    let history = pending(&adapter.client, approval)
        .await
        .map_err(|error| fault(error, None))?;
    let Some(page) = history else {
        return Ok(());
    };
    let detail = adapter
        .client
        .tool_detail(
            call.detail_reference(&page)
                .ok_or_else(|| invalid("approval tool detail identity is unavailable"))?,
        )
        .await
        .map_err(|error| fault(error, None))?;
    let hydrated = call.with_detail(detail);
    let call = &hydrated;
    let tool = acp::ToolCallUpdate::new(
        call_id(*source, call),
        acp::ToolCallUpdateFields::new()
            .title(title(call, &page))
            .status(acp::ToolCallStatus::Pending)
            .raw_input(call.arguments(&page).cloned()),
    );
    let request = acp::RequestPermissionRequest::new(
        acp::SessionId::new(opened.session.lock().await.id.to_string()),
        tool,
        vec![
            acp::PermissionOption::new("allow", "Allow once", acp::PermissionOptionKind::AllowOnce),
            acp::PermissionOption::new("deny", "Deny", acp::PermissionOptionKind::RejectOnce),
        ],
    )
    .meta(metadata(
        json!({"node":adapter.client.target(),"session":source,"turn":approval.turn,
            "approval":approval,"decision_request_id":decision}),
    ));
    let mut observation = if *source == opened.session.lock().await.id {
        opened.view.clone()
    } else {
        opened
            .presentation
            .lock()
            .await
            .views
            .get(source)
            .cloned()
            .ok_or_else(|| invalid("child observation is unavailable"))?
    };
    let request = connection.send_request(request);
    operation["rpc_request_id"] = json!(request.id());
    let response = request.block_task();
    tokio::pin!(response);
    let response = loop {
        tokio::select! {
            _ = opened.stop.cancelled() => return Ok(()),
            response = &mut response => break response?,
            changed = observation.changed() => {
                if changed.is_err() { return Ok(()); }
                let current = observation.borrow_and_update().clone();
                if current.connected && current.snapshot.as_ref().is_some_and(|snapshot|
                    !snapshot.page.approvals.iter().any(|current|
                        current.id == approval.id && current.state == ApprovalState::Pending)) {
                    return Ok(());
                }
            },
        }
    };
    let selected = match response.outcome {
        acp::RequestPermissionOutcome::Selected(selected)
            if selected.option_id.to_string() == "allow" =>
        {
            sailry_protocol::conversation::Decision::Approve
        }
        acp::RequestPermissionOutcome::Selected(selected)
            if selected.option_id.to_string() == "deny" =>
        {
            sailry_protocol::conversation::Decision::Deny
        }
        acp::RequestPermissionOutcome::Cancelled => return Ok(()),
        _ => return Err(invalid("permission choice is unavailable")),
    };
    if pending(&adapter.client, approval)
        .await
        .map_err(|error| fault(error, None))?
        .is_none()
    {
        return Ok(());
    }
    // One editor choice resolves the captured Node approval; lost receipt is never replayed.
    let mut request = adapter.client.prepare(Command::ResolveApproval {
        session: *source,
        approval: approval.id,
        decision: selected,
    });
    request.id = *decision;
    operation["request_id"] = json!(decision);
    operation["state"] = json!("resolving");
    adapter
        .client
        .execute(request)
        .await
        .map_err(|error| fault(error, Some(*decision)))?;
    Ok(())
}

async fn pending(
    client: &Client,
    approval: &Approval,
) -> std::result::Result<Option<sailry_protocol::conversation::Page>, Fault> {
    let mut before = None;
    loop {
        let history = client
            .read_conversation(approval.session, before, 100)
            .await?;
        if history
            .page
            .runs
            .iter()
            .any(|run| run.turn == approval.turn)
        {
            return Ok(history
                .page
                .approvals
                .iter()
                .any(|current| {
                    current.id == approval.id
                        && current.turn == approval.turn
                        && current.entry == approval.entry
                        && current.index == approval.index
                        && current.state == ApprovalState::Pending
                })
                .then_some(history.page));
        }
        match history.page.next_before {
            Some(cursor) if Some(cursor) != before => before = Some(cursor),
            _ => return Ok(None),
        }
    }
}
