//! Observe canonical Agent state; admitting a turn is not task completion.
use crate::store::Ingress;
use sailry_link::Handler;
use sailry_protocol::{conversation::Status, dispatch::Completion, *};

pub(crate) fn turn_result(status: Status) -> Option<Result<(), Fault>> {
    Some(match status {
        Status::Queued | Status::Running | Status::Stopping => return None,
        Status::Completed => Ok(()),
        // This observer owns the admitted turn, not a successor invocation.
        Status::Continued => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "task turn continued in another invocation",
        )),
        Status::Cancelled => Err(Fault::new(ErrorCode::Cancelled, "task turn was cancelled")),
        Status::Interrupted => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "task turn was interrupted",
        )),
        Status::Failed => Err(Fault::new(ErrorCode::Internal, "task turn failed")),
    })
}

pub(super) async fn wait(
    ingress: &Ingress,
    completion: Completion,
    output: Result<Output, Fault>,
) -> Result<(), Fault> {
    let output = output?;
    if completion == Completion::Command {
        return Ok(());
    }
    let Output::QueuedTurn(turn) = output else {
        return Err(Fault::invalid("turn callback did not admit a turn"));
    };
    loop {
        let mut feed = ingress
            .subscribe(ingress.node, Topic::Conversation(turn.session))
            .await?;
        loop {
            let run = match feed.next().await? {
                Update::ConversationSnapshot(snapshot) => {
                    if let Some(run) = snapshot.page.runs.iter().find(|run| run.turn == turn.id) {
                        Some(run.clone())
                    } else {
                        let request = Request::new(
                            ingress.node,
                            Command::ReadTurn {
                                session: turn.session,
                                turn: turn.id,
                                expected_revision: snapshot.page.revision,
                                before: None,
                                limit: 1,
                            },
                        );
                        let output = ingress
                            .dispatch_internal(request)
                            .await?
                            .completion
                            .await
                            .map_err(|_| {
                                Fault::new(ErrorCode::OutcomeUnknown, "task state was not received")
                            })?;
                        match output {
                            Ok(Output::TurnHistory(history)) => Some(history.run),
                            Err(error) if error.code == ErrorCode::RevisionConflict => break,
                            Err(error) => return Err(error),
                            _ => {
                                return Err(Fault::new(
                                    ErrorCode::Internal,
                                    "unexpected task state",
                                ));
                            }
                        }
                    }
                }
                Update::ConversationFrame(frame) => match frame.change {
                    conversation::Change::Run(run) if run.turn == turn.id => Some(run),
                    _ => None,
                },
                Update::ResetRequired => break,
                _ => None,
            };
            if let Some(run) = run
                && let Some(result) = turn_result(run.status)
            {
                return result.map_err(|error| run.error.unwrap_or(error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_termination() {
        assert_eq!(turn_result(Status::Completed), Some(Ok(())));
        assert_eq!(
            turn_result(Status::Continued),
            Some(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "task turn continued in another invocation",
            )))
        );
        assert_eq!(
            turn_result(Status::Cancelled),
            Some(Err(Fault::new(
                ErrorCode::Cancelled,
                "task turn was cancelled",
            )))
        );
        for status in [Status::Queued, Status::Running, Status::Stopping] {
            assert_eq!(turn_result(status), None);
        }
    }
}
