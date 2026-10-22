//! Durable authorization metadata, alongside the single canonical ADK history.
use super::calls::{active, id as call_id, matches as matches_call};
use super::*;
use adk_core::ToolConfirmationRequest;

mod access;
mod prefix;
mod prepared;
pub(in crate::store) use prepared::Prepared;
pub(crate) mod results;
mod rules;
pub(crate) use rules::Binding;
mod waiting;
pub(in crate::store) use waiting::{Waiting, cancel, check_waiter, prune};
#[cfg(any(test, feature = "test-support"))]
pub(in crate::store) mod fixture;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Capture<'a> {
    pub(super) startup: Option<&'a sailry_protocol::code::Startup>,
    pub(super) ssh: Option<&'a sailry_protocol::ssh::Trust>,
    pub(super) prepared: Option<&'a Command>,
}

impl Ingress {
    pub(crate) async fn approved_command_access(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
    ) -> Result<sailry_protocol::process::Access, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::CommandAccess {
                turn,
                request,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn confirm(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        binding: Option<Binding>,
    ) -> Result<Decision, Fault> {
        let (reply, response) = oneshot::channel();
        let pending = waiting::Pending::new(self, turn, &request)?;
        let _waiting = self
            .agents
            .timeline(turn)
            .map(|timeline| timeline.span(crate::agent::observation::Phase::Excluded));
        self.sender
            .send(Job::Agent(Box::new(Operation::Confirm {
                turn,
                owner: pending.owner,
                deadline: pending.deadline,
                request,
                binding,
                startup: None,
                ssh: None,
                prepared: None,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        pending.wait(response).await
    }

    pub(crate) async fn confirm_code(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        binding: Binding,
        startup: sailry_protocol::code::Startup,
    ) -> Result<Decision, Fault> {
        let root = self.worktree_root(startup.worktree).await?;
        self.code.validate(&startup, &root).await?;
        let (reply, response) = oneshot::channel();
        let pending = waiting::Pending::new(self, turn, &request)?;
        let _waiting = self
            .agents
            .timeline(turn)
            .map(|timeline| timeline.span(crate::agent::observation::Phase::Excluded));
        self.sender
            .send(Job::Agent(Box::new(Operation::Confirm {
                turn,
                owner: pending.owner,
                deadline: pending.deadline,
                request,
                binding: Some(binding),
                startup: Some(startup),
                ssh: None,
                prepared: None,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        pending.wait(response).await
    }

    pub(crate) async fn confirm_connection(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        binding: Binding,
        prepared: Command,
        ssh: Option<sailry_protocol::ssh::Trust>,
    ) -> Result<Decision, Fault> {
        let (reply, response) = oneshot::channel();
        let pending = waiting::Pending::new(self, turn, &request)?;
        let _waiting = self
            .agents
            .timeline(turn)
            .map(|timeline| timeline.span(crate::agent::observation::Phase::Excluded));
        self.sender
            .send(Job::Agent(Box::new(Operation::Confirm {
                turn,
                owner: pending.owner,
                deadline: pending.deadline,
                request,
                binding: Some(binding),
                startup: None,
                ssh,
                prepared: Some(prepared),
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        pending.wait(response).await
    }

    pub(crate) async fn authorize(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        prepared: Option<Command>,
    ) -> Result<RequestId, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Authorize {
                turn,
                request,
                prepared,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn approved_connection(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        prepared: Command,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ConnectionAccess {
                turn,
                request,
                prepared,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn denials(&self, turn: TurnId) -> Result<Vec<results::Denial>, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Denials { turn, reply })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn command_access(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
) -> Result<sailry_protocol::process::Access, Fault> {
    active(db, turn)?;
    super::calls::locate(db, turn, request)?;
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_approvals WHERE turn=?1 AND call=?2",
            params![turn.to_string(), call_id(request)?],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let approval: Approval = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "command has no approval"))?;
    if approval.state != ApprovalState::Approved {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "command approval is unavailable",
        ));
    }
    approval.access.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "command approval has no captured access",
        )
    })
}

#[cfg(test)]
pub(super) fn begin(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    binding: Option<&Binding>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Approval, Fault> {
    begin_with(database, turn, request, binding, None, events)
}

#[cfg(test)]
pub(super) fn begin_with(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    binding: Option<&Binding>,
    startup: Option<&sailry_protocol::code::Startup>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Approval, Fault> {
    begin_for(
        database,
        turn,
        request,
        binding,
        Capture {
            startup,
            ..Default::default()
        },
        events,
    )
}

pub(super) fn begin_for(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    binding: Option<&Binding>,
    capture: Capture<'_>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Approval, Fault> {
    let Capture {
        startup,
        ssh,
        prepared,
    } = capture;
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let run = active(&transaction, turn)?;
    let call = call_id(request)?;
    let (entry, index) = super::calls::locate(&transaction, turn, request)?;
    let mut source = if let Some(ssh) = ssh {
        super::connections::validate_trust(&transaction, turn, binding, ssh)?;
        if permissions::config(&transaction, turn)?.mode == WorkMode::Plan {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "planning turns cannot trust SSH host keys",
            ));
        }
        ApprovalSource::User
    } else if let Some(startup) = startup {
        super::permissions::code_source(&transaction, turn, request, startup)?
    } else {
        super::permissions::source(&transaction, turn, request)?
    };
    if let Some(binding) = binding {
        rules::validate(&transaction, turn, request, binding)?;
    }
    let prepared = match prepared {
        Some(command) => Prepared::new(command)?,
        None => prepared::baseline(&transaction, turn, binding)?,
    };
    let access = access::requested(&transaction, turn, request, binding)?;
    let escape = crate::process::sandbox::SUPPORTED
        && access == Some(sailry_protocol::process::Access::Unrestricted);
    if escape && source != ApprovalSource::Full {
        source = ApprovalSource::User;
    }
    let can_remember = startup.is_none()
        && ssh.is_none()
        && binding.is_some()
        && source == ApprovalSource::User
        && !escape;
    let prefix = binding
        .and_then(|binding| rules::command(binding, request))
        .and_then(prefix::offered)
        .filter(|_| can_remember);
    let mut choice = None;
    if can_remember
        && let Some(binding) = binding
        && let Some((matched, remembered)) = rules::matching(&transaction, &run, binding, request)?
    {
        source = matched;
        choice = Some(remembered);
    }
    let approval = Approval {
        id: ApprovalId::new(),
        session: run.session,
        turn,
        entry,
        index,
        state: if source == ApprovalSource::User {
            ApprovalState::Pending
        } else {
            ApprovalState::Approved
        },
        source,
        choice,
        reason: None,
        prefix,
        can_remember,
        access,
        startup: startup.cloned(),
        ssh: ssh.cloned(),
    };
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_approvals WHERE turn=?1 AND call=?2)",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "tool call already requested approval",
        ));
    }
    transaction.execute("INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed,prepared) VALUES(?1,?2,?3,?4,?5,?6,0,?7)",
        params![approval.id.to_string(), turn.to_string(), call, encode(&approval)?, encode(&binding)?, RequestId::new().to_string(), encode(&prepared)?]).map_err(storage_error)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database.feeds.publish(
        database.node,
        run.session,
        Change::Approval(approval.clone()),
    );
    Ok(approval)
}

pub(in crate::store) fn resolve(
    db: &Connection,
    session: SessionId,
    id: ApprovalId,
    decision: Decision,
) -> Result<Approval, Fault> {
    let mut approval = get(db, id)?;
    if approval.session != session {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "approval belongs to another session",
        ));
    }
    if approval.state != ApprovalState::Pending {
        if matches_decision(&approval, &decision) {
            return Ok(approval);
        }
        return Err(Fault::new(
            ErrorCode::Conflict,
            "approval has already closed",
        ));
    }
    let (state, choice, reason) = match decision {
        Decision::Approve => (ApprovalState::Approved, Some(ApprovalChoice::Once), None),
        Decision::Deny => (ApprovalState::Denied, None, None),
        Decision::ApproveSession => (ApprovalState::Approved, Some(ApprovalChoice::Session), None),
        Decision::ApprovePrefix(prefix) => {
            let request = request(db, &approval)?;
            let binding = rules::read(db, id)?
                .ok_or_else(|| Fault::invalid("tool approval cannot remember a prefix"))?;
            let command = rules::command(&binding, &request)
                .ok_or_else(|| Fault::invalid("tool approval has no shell command"))?;
            let prefix = prefix::select(command, &prefix)?;
            (
                ApprovalState::Approved,
                Some(ApprovalChoice::Prefix(prefix)),
                None,
            )
        }
        Decision::DenyWithReason(reason) => {
            results::validate_reason(&reason)?;
            (ApprovalState::Denied, None, Some(reason))
        }
    };
    let run = active(db, approval.turn)?;
    if let Some(choice @ (ApprovalChoice::Session | ApprovalChoice::Prefix(_))) = &choice {
        if !approval.can_remember || approval.source != ApprovalSource::User {
            return Err(Fault::invalid("tool approval cannot be remembered"));
        }
        let binding = rules::read(db, id)?
            .ok_or_else(|| Fault::invalid("tool approval has no captured owner"))?;
        rules::validate(db, approval.turn, &request(db, &approval)?, &binding)?;
        rules::insert(db, &run, &binding, choice, id)?;
    }
    approval.state = state;
    approval.choice = choice;
    approval.reason = reason;
    put(db, &approval)?;
    Ok(approval)
}

/// Claims the decision immediately before the actor commits its transaction.
/// Expiry can only be answered by this actor after the commit attempt has finished.
pub(in crate::store) fn claim(
    db: &Connection,
    waiting: &Waiting,
    output: &Output,
) -> Result<(), Fault> {
    let Output::Approval(approval) = output else {
        return Ok(());
    };
    check_waiter(waiting, approval.id)?;
    if approval.choice == Some(ApprovalChoice::Session) {
        let run = runs::get(db, approval.turn)?;
        // Repeating an already committed choice remains idempotent after its run
        // ends. New decisions were checked by resolve in this same transaction.
        if run.status != Status::Running {
            return Ok(());
        }
        let binding = rules::read(db, approval.id)?
            .ok_or_else(|| Fault::invalid("tool approval has no captured owner"))?;
        rules::apply_session(db, waiting, &run, &binding)?;
    }
    Ok(())
}

fn matches_decision(approval: &Approval, decision: &Decision) -> bool {
    let (state, choice, reason) = match decision {
        Decision::Approve => (ApprovalState::Approved, Some(ApprovalChoice::Once), None),
        Decision::ApproveSession => (ApprovalState::Approved, Some(ApprovalChoice::Session), None),
        Decision::ApprovePrefix(value) => {
            let Some(words) = prefix::words(value) else {
                return false;
            };
            (
                ApprovalState::Approved,
                Some(ApprovalChoice::Prefix(words.join(" "))),
                None,
            )
        }
        Decision::Deny => (ApprovalState::Denied, None, None),
        Decision::DenyWithReason(value) => (ApprovalState::Denied, None, Some(value.as_str())),
    };
    approval.state == state && approval.choice == choice && approval.reason.as_deref() == reason
}

pub(super) fn connection_access(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    prepared: &Command,
) -> Result<(), Fault> {
    active(db, turn)?;
    let (entry, index) = super::calls::locate(db, turn, request)?;
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_approvals WHERE turn=?1 AND call=?2",
            params![turn.to_string(), call_id(request)?],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let approval: Approval = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "connection has no approval"))?;
    if approval.state != ApprovalState::Approved
        || approval.entry != entry
        || approval.index != index
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "connection approval is unavailable",
        ));
    }
    prepared::check_connection(db, &approval, Some(prepared))
}

pub(super) fn authorize(
    db: &mut Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    prepared: Option<&Command>,
) -> Result<RequestId, Fault> {
    let transaction = db.transaction().map_err(storage_error)?;
    active(&transaction, turn)?;
    let record: Option<(Vec<u8>, String, bool)> = transaction
        .query_row(
            "SELECT body,request,claimed FROM agent_approvals WHERE turn=?1 AND call=?2",
            params![turn.to_string(), call_id(request)?],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((body, id, claimed)) = record else {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "tool call has no approval",
        ));
    };
    let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
    if approval.state != ApprovalState::Approved || claimed {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "tool approval is unavailable or already used",
        ));
    }
    let body: Vec<u8> = transaction
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![turn.to_string(), approval.entry],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if !event
        .content()
        .and_then(|content| content.parts.get(approval.index))
        .is_some_and(|part| matches_call(part, request))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "tool arguments differ from the approved call",
        ));
    }
    prepared::check_connection(&transaction, &approval, prepared)?;
    access::check(&approval, prepared)?;
    if let Some(startup) = &approval.startup {
        if !matches!(prepared, Some(Command::StartCodeServer { startup: actual }) if actual == startup)
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "prepared language startup differs from approval",
            ));
        }
        super::super::code::grant(&transaction, startup)?;
    }
    if let Some(ssh) = &approval.ssh
        && !matches!(prepared, Some(Command::RunSsh { profile, expected_revision, .. } | Command::TransferSsh { profile, expected_revision, .. }) if *profile == ssh.profile && *expected_revision == ssh.expected_revision)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "prepared SSH connection differs from host key approval",
        ));
    }
    rules::check_prepared(&approval, prepared)?;
    let previous: Vec<u8> = transaction
        .query_row(
            "SELECT prepared FROM agent_approvals WHERE id=?1",
            [approval.id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let baseline: Option<Prepared> = serde_json::from_slice(&previous).map_err(storage_error)?;
    let mut captured = prepared.map(Prepared::new).transpose()?.flatten();
    if let (Some(captured), Some(baseline)) = (&mut captured, baseline) {
        captured.targets = baseline.targets;
    }
    transaction
        .execute(
            "UPDATE agent_approvals SET claimed=1,prepared=?2 WHERE id=?1",
            params![approval.id.to_string(), encode(&captured)?],
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    id.parse().map_err(storage_error)
}

pub(super) fn close(
    db: &Connection,
    turn: TurnId,
    state: ApprovalState,
) -> Result<Vec<Approval>, Fault> {
    let mut query = db.prepare("SELECT body FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='pending'").map_err(storage_error)?;
    let mut approvals = query
        .query_map([turn.to_string()], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|body| {
            serde_json::from_slice::<Approval>(&body.map_err(storage_error)?).map_err(storage_error)
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    for approval in &mut approvals {
        approval.state = state;
        put(db, approval)?;
    }
    Ok(approvals)
}

pub(super) fn list(
    db: &Connection,
    session: SessionId,
    entries: &[Entry],
) -> Result<Vec<Approval>, Fault> {
    let ids: Vec<_> = entries.iter().map(|entry| &entry.id).collect();
    let mut query = db.prepare("SELECT a.body FROM agent_approvals a JOIN conversation_turns h ON h.turn=a.turn WHERE h.session=?1 AND (json_extract(a.body,'$.state')='pending' OR json_extract(a.body,'$.entry') IN (SELECT value FROM json_each(?2))) ORDER BY a.rowid").map_err(storage_error)?;
    query
        .query_map(
            params![
                session.to_string(),
                serde_json::to_string(&ids).map_err(storage_error)?
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?
        .map(|body| {
            let mut approval: Approval =
                serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
            approval.session = session;
            Ok(approval)
        })
        .collect()
}

pub(super) fn get(db: &Connection, id: ApprovalId) -> Result<Approval, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_approvals WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "approval does not exist"))?,
    )
    .map_err(storage_error)
}

fn put(db: &Connection, approval: &Approval) -> Result<(), Fault> {
    db.execute(
        "UPDATE agent_approvals SET body=?2 WHERE id=?1",
        params![approval.id.to_string(), encode(approval)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn request(
    db: &Connection,
    approval: &Approval,
) -> Result<ToolConfirmationRequest, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![approval.turn.to_string(), approval.entry],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    let Some(adk_core::Part::FunctionCall { id, name, args, .. }) = event
        .content()
        .and_then(|content| content.parts.get(approval.index))
    else {
        return Err(Fault::invalid("approval has no canonical tool call"));
    };
    Ok(ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: name.clone(),
        function_call_id: id.clone(),
        args: args.clone(),
    })
}
