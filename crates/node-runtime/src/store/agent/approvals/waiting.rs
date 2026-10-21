//! A handler's lifetime owns its pending Node approval, including ADK timeout drops.
use super::*;
use std::collections::BTreeMap;

pub(in crate::store) type Waiting =
    BTreeMap<ApprovalId, (TurnId, RequestId, oneshot::Sender<Result<Decision, Fault>>)>;

pub(super) struct Pending {
    sender: tokio::sync::mpsc::Sender<Job>,
    turn: TurnId,
    call: String,
    pub(super) owner: RequestId,
    complete: bool,
}

impl Pending {
    pub(super) fn new(
        ingress: &Ingress,
        turn: TurnId,
        request: &ToolConfirmationRequest,
    ) -> Result<Self, Fault> {
        Ok(Self {
            sender: ingress.sender.clone(),
            turn,
            call: call_id(request)?.into(),
            owner: RequestId::new(),
            complete: false,
        })
    }

    pub(super) fn complete(&mut self) {
        self.complete = true;
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let sender = self.sender.clone();
        let turn = self.turn;
        let call = self.call.clone();
        let owner = self.owner;
        tokio::spawn(async move {
            let _ = sender
                .send(Job::Agent(Box::new(Operation::CancelApproval {
                    turn,
                    call,
                    owner,
                })))
                .await;
        });
    }
}

pub(in crate::store) fn check_waiter(waiting: &Waiting, id: ApprovalId) -> Result<(), Fault> {
    if waiting
        .get(&id)
        .is_some_and(|(_, _, reply)| reply.is_closed())
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "approval is no longer awaited",
        ));
    }
    Ok(())
}

pub(in crate::store) fn prune(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let closed: Vec<_> = database
        .approvals
        .iter()
        .filter(|(_, (_, _, reply))| reply.is_closed())
        .map(|(id, _)| *id)
        .collect();
    for id in closed {
        close(database, id, events)?;
    }
    Ok(())
}

pub(in crate::store) fn cancel(
    database: &mut Database,
    turn: TurnId,
    call: &str,
    owner: RequestId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let id: Option<String> = database
        .connection
        .query_row(
            "SELECT id FROM agent_approvals WHERE turn=?1 AND call=?2",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let Some(id) = id else { return Ok(()) };
    let id = id.parse().map_err(storage_error)?;
    if database
        .approvals
        .get(&id)
        .is_some_and(|(active_turn, active_owner, _)| {
            *active_turn == turn && *active_owner == owner
        })
    {
        close(database, id, events)?;
    }
    Ok(())
}

fn close(
    database: &mut Database,
    id: ApprovalId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut approval = get(&transaction, id)?;
    if approval.state != ApprovalState::Pending {
        return Ok(());
    }
    approval.state = ApprovalState::Cancelled;
    put(&transaction, &approval)?;
    let envelope = changed(&transaction, database.node, approval.session)?;
    transaction.commit().map_err(storage_error)?;
    database.approvals.remove(&id);
    let _ = events.send(envelope);
    database
        .feeds
        .publish(database.node, approval.session, Change::Approval(approval));
    Ok(())
}
