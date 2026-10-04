//! Durable authorization metadata, alongside the single canonical ADK history.
use super::calls::{active, id as call_id, matches as matches_call};
use super::*;
use adk_core::ToolConfirmationRequest;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub(in crate::store) type Waiting =
    BTreeMap<ApprovalId, (TurnId, oneshot::Sender<Result<Decision, Fault>>)>;

impl Ingress {
    pub(crate) async fn confirm(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
    ) -> Result<Decision, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Confirm {
                turn,
                request,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn authorize(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
    ) -> Result<RequestId, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Authorize {
                turn,
                request,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn begin(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Approval, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let run = active(&transaction, turn)?;
    let call = call_id(request)?;
    let (entry, index) = super::calls::locate(&transaction, turn, request)?;
    let source = super::permissions::source(&transaction, turn, request)?;
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
    transaction.execute("INSERT INTO agent_approvals(id,turn,call,body,request,claimed) VALUES(?1,?2,?3,?4,?5,0)",
        params![approval.id.to_string(), turn.to_string(), call, encode(&approval)?, RequestId::new().to_string()]).map_err(storage_error)?;
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
    let state = match decision {
        Decision::Approve => ApprovalState::Approved,
        Decision::Deny => ApprovalState::Denied,
    };
    if approval.state == state {
        return Ok(approval);
    }
    if approval.state != ApprovalState::Pending {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "approval has already closed",
        ));
    }
    active(db, approval.turn)?;
    approval.state = state;
    put(db, &approval)?;
    Ok(approval)
}

pub(super) fn authorize(
    db: &mut Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
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
    transaction
        .execute(
            "UPDATE agent_approvals SET claimed=1 WHERE id=?1",
            [approval.id.to_string()],
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

fn get(db: &Connection, id: ApprovalId) -> Result<Approval, Fault> {
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
