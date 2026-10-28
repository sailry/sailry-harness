//! Retain execution ownership until the existing durable terminal write succeeds.
use super::*;

pub(super) async fn finish(
    ingress: &Ingress,
    turn: TurnId,
    session: SessionId,
    status: Status,
    error: Option<Fault>,
    owner: Option<CancellationToken>,
    review: Option<canonical::Record>,
) -> Result<Status, Fault> {
    let mut reported = false;
    loop {
        let failure = match ingress
            .finish_run_with(turn, status, error.clone(), owner.clone(), review.clone())
            .await
        {
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
