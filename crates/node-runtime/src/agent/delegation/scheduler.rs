use super::*;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

pub(in crate::agent) async fn run(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: Invocation,
    token: CancellationToken,
    owner: CancellationToken,
) -> Result<(Status, Option<String>), Fault> {
    let turn = invocation.turn.id;
    let session = invocation.turn.session;
    let status = match wait(ingress, turn, session, &owner, &token).await {
        Ok(status) => status,
        Err(fault) => {
            super::super::settlement::queued(
                ingress,
                turn,
                session,
                Status::Failed,
                Some(fault.clone()),
            )
            .await?;
            ingress.agents.finish(turn);
            ingress.agents.wake.notify_waiters();
            return Err(fault);
        }
    };
    if status != Status::Running {
        return Ok((status, None));
    }
    match AssertUnwindSafe(super::super::run(
        ingress,
        tasks,
        invocation,
        token,
        Some(owner),
    ))
    .catch_unwind()
    .await
    {
        Ok(result) => result,
        Err(_) => {
            // Panics must neither expose their payload nor strand a durable slot.
            // Cancel and drain owned effects before publishing the terminal run.
            ingress.agents.cancel(turn);
            ingress.agents.drain(turn).await;
            let fault = Fault::new(
                ErrorCode::OutcomeUnknown,
                "child execution failed unexpectedly; calls were not replayed",
            );
            super::super::settlement::finish(
                ingress,
                turn,
                session,
                Status::Failed,
                Some(fault.clone()),
                None,
                None,
            )
            .await?;
            ingress.agents.finish(turn);
            ingress.agents.wake.notify_waiters();
            Err(fault)
        }
    }
}

async fn wait(
    ingress: &Ingress,
    turn: TurnId,
    session: SessionId,
    owner: &CancellationToken,
    token: &CancellationToken,
) -> Result<Status, Fault> {
    loop {
        let notified = ingress.agents.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ingress.agents.stopped.is_cancelled() || owner.is_cancelled() || token.is_cancelled() {
            let status = if ingress.agents.stopped.is_cancelled() {
                Status::Interrupted
            } else {
                Status::Cancelled
            };
            let status =
                super::super::settlement::queued(ingress, turn, session, status, None).await?;
            ingress.agents.finish(turn);
            ingress.agents.wake.notify_waiters();
            return Ok(status);
        }
        let status = ingress.claim_child(turn, token.clone()).await?;
        if status != Status::Queued {
            if status == Status::Running {
                // Another eligible waiter may fill the remaining Node capacity.
                ingress.agents.wake.notify_waiters();
            } else {
                ingress.agents.finish(turn);
            }
            return Ok(status);
        }
        tokio::select! {
            _ = ingress.agents.stopped.cancelled() => {},
            _ = owner.cancelled() => {},
            _ = token.cancelled() => {},
            _ = notified => {},
        }
    }
}
