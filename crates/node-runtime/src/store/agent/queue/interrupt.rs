use super::*;

pub(in crate::store) async fn deliver(
    database: &Database,
    controls: &crate::agent::Controls,
    output: &Output,
    recipient: TurnId,
    request: RequestId,
) -> Result<(), Fault> {
    let selected = match output {
        Output::QueuedTurn(turn) => read(&database.connection, turn.session)
            .await?
            .items
            .iter()
            .any(|item| item.turn == turn.id && item.guidance == Some(recipient)),
        Output::Queue(update) => read(&database.connection, update.session)
            .await?
            .items
            .iter()
            .any(|item| {
                item.guidance == Some(recipient)
                    && update.runs.iter().any(|run| run.turn == item.turn)
            }),
        _ => false,
    };
    if !selected {
        return Ok(());
    }
    controls.guidance(recipient, request);
    Ok(())
}
