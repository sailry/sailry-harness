//! Retain execution ownership until the existing durable terminal write succeeds.
use super::*;

pub(in crate::agent) async fn finish(
    ingress: &Ingress,
    turn: TurnId,
    session: SessionId,
    status: Status,
    error: Option<Fault>,
    owner: Option<CancellationToken>,
    review: Option<canonical::Record>,
) -> Result<Status, Fault> {
    settle(
        ingress,
        turn,
        session,
        status,
        error,
        Pending::Running { owner, review },
    )
    .await
}

pub(in crate::agent) async fn queued(
    ingress: &Ingress,
    turn: TurnId,
    session: SessionId,
    status: Status,
    error: Option<Fault>,
) -> Result<Status, Fault> {
    settle(ingress, turn, session, status, error, Pending::Queued).await
}

enum Pending {
    Running {
        owner: Option<CancellationToken>,
        review: Option<canonical::Record>,
    },
    Queued,
}

async fn settle(
    ingress: &Ingress,
    turn: TurnId,
    session: SessionId,
    status: Status,
    error: Option<Fault>,
    pending: Pending,
) -> Result<Status, Fault> {
    let mut reported = false;
    loop {
        let result = match &pending {
            Pending::Running { owner, review } => {
                ingress
                    .finish_run_with(turn, status, error.clone(), owner.clone(), review.clone())
                    .await
            }
            Pending::Queued => {
                ingress
                    .finish_queued_child(turn, status, error.clone())
                    .await
            }
        };
        let failure = match result {
            Ok(status) => return Ok(status),
            Err(error) => error,
        };
        if !reported {
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Run,
                crate::logging::Phase::Failed,
            );
            entry.context = crate::logging::Context {
                node: Some(ingress.node),
                session: Some(session),
                turn: Some(turn),
                ..Default::default()
            };
            entry.reason = Some("finish");
            entry.code = Some(failure.code);
            entry.emit();
            reported = true;
        }
        // This repeats only settlement. The runner, tools, hooks and cleanup have
        // already ended and remain sealed in Controls while storage recovers.
        tokio::select! {
            biased;
            _ = ingress.agents.stopped.cancelled() => return Err(failure),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    }
}
