//! The storage actor settles approval expiry and durable decisions in one order.
use super::*;
use std::{collections::BTreeMap, time::Instant};

pub(in crate::store) type Waiting = BTreeMap<
    ApprovalId,
    (
        TurnId,
        RequestId,
        Instant,
        Option<oneshot::Sender<Result<Decision, Fault>>>,
    ),
>;

pub(super) struct Pending {
    sender: tokio::sync::mpsc::Sender<Job>,
    turn: TurnId,
    call: String,
    pub(super) owner: RequestId,
    pub(super) deadline: Instant,
    #[cfg(any(test, feature = "test-support"))]
    expiry: Arc<tokio::sync::Notify>,
    complete: bool,
}

impl Pending {
    pub(super) fn new(
        ingress: &Ingress,
        turn: TurnId,
        request: &ToolIdentity,
    ) -> Result<Self, Fault> {
        let timeout = super::TIMEOUT;
        #[cfg(any(test, feature = "test-support"))]
        let timeout = ingress
            .agents
            .approval_timeout
            .lock()
            .unwrap()
            .unwrap_or(timeout);
        Ok(Self {
            deadline: Instant::now() + timeout,
            #[cfg(any(test, feature = "test-support"))]
            expiry: ingress.agents.approval_expiry.clone(),
            sender: ingress.sender.clone(),
            turn,
            call: call_id(request)?.into(),
            owner: RequestId::new(),
            complete: false,
        })
    }

    fn cancellation(&self) -> Job {
        Job::Agent(Box::new(Operation::CancelApproval {
            turn: self.turn,
            call: self.call.clone(),
            owner: self.owner,
        }))
    }

    pub(super) async fn wait(
        mut self,
        mut response: oneshot::Receiver<Result<Decision, Fault>>,
    ) -> Result<Decision, Fault> {
        let result = tokio::select! {
            decision = &mut response => decision,
            _ = tokio::time::sleep_until(self.deadline.into()) => {
                // Expiry asks the same owner to settle. A commit already claimed before
                // the deadline must finish before this wait can become a denial.
                self.sender.send(self.cancellation()).await.map_err(|_| super::super::super::unavailable())?;
                #[cfg(any(test, feature = "test-support"))]
                self.expiry.notify_one();
                response.await
            }
        };
        self.complete = true;
        result.map_err(|_| super::super::super::unavailable())?
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        let sender = self.sender.clone();
        let job = self.cancellation();
        tokio::spawn(async move {
            let _ = sender.send(job).await;
        });
    }
}

pub(in crate::store) fn check_waiter(
    waiting: &std::sync::Mutex<Waiting>,
    id: ApprovalId,
) -> Result<(), Fault> {
    if waiting
        .lock()
        .unwrap()
        .get(&id)
        .is_some_and(|(_, _, deadline, reply)| {
            reply.as_ref().is_none_or(oneshot::Sender::is_closed) || Instant::now() >= *deadline
        })
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "approval is no longer awaited",
        ));
    }
    Ok(())
}

pub(in crate::store) async fn prune(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let closed: Vec<_> = database
        .approvals
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, (_, _, deadline, reply))| {
            reply.as_ref().is_none_or(oneshot::Sender::is_closed) || Instant::now() >= *deadline
        })
        .map(|(id, _)| *id)
        .collect();
    for id in closed {
        close(database, id, events).await?;
    }
    Ok(())
}

pub(in crate::store) async fn cancel(
    database: &mut Database,
    turn: TurnId,
    call: &str,
    owner: RequestId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let result: Result<(), Fault> = (async || {
        let id: Option<String> = {
            let mut statement = database
                .connection
                .prepare("SELECT id FROM agent_approvals WHERE turn=?1 AND call=?2")
                .await
                .map_err(storage_error)?;
            match statement.query_row(params![turn.to_string(), call]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        let Some(id) = id else { return Ok(()) };
        let id = id.parse().map_err(storage_error)?;
        let awaited = database.approvals.lock().unwrap().get(&id).is_some_and(
            |(active_turn, active_owner, _, _)| *active_turn == turn && *active_owner == owner,
        );
        if awaited {
            close(database, id, events).await?;
        }
        Ok(())
    })()
    .await;
    if let Err(error) = &result {
        // Keep this closed owner until durable cleanup succeeds. Removing it here
        // would let a late decision treat the still-Pending record as unawaited.
        for (active_turn, active_owner, _, reply) in database.approvals.lock().unwrap().values_mut()
        {
            if *active_turn == turn
                && *active_owner == owner
                && let Some(reply) = reply.take()
            {
                let _ = reply.send(Err(error.clone()));
            }
        }
    }
    result
}

async fn close(
    database: &mut Database,
    id: ApprovalId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let prepared: Result<_, Fault> = async {
            let mut approval = get(&transaction, id).await?;
            if approval.state != ApprovalState::Pending {
                return Ok(None);
            }
            approval.state = ApprovalState::Cancelled;
            put(&transaction, &approval).await?;
            let envelope = changed(&transaction, database.node, approval.session).await?;
            Ok(Some((approval, envelope)))
        }
        .await;
        let Some((approval, envelope)) = (match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                transaction.rollback().await.map_err(storage_error)?;
                return Err(error);
            }
        }) else {
            transaction.rollback().await.map_err(storage_error)?;
            return Ok(());
        };
        #[cfg(any(test, feature = "test-support"))]
        super::fixture::wait(&database.approval_commit, true, &transaction).await;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        if let Some((_, _, _, Some(reply))) = database.approvals.lock().unwrap().remove(&id) {
            let _ = reply.send(Ok(Decision::Deny));
        }
        publication.send(events, envelope);
        database
            .feeds
            .publish(database.node, approval.session, Change::Approval(approval));
        Ok(())
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
