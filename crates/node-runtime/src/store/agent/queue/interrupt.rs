use super::*;

pub(in crate::store) async fn deliver(
    database: &mut Database,
    controls: &crate::agent::Controls,
    output: &Output,
    recipient: TurnId,
    request: RequestId,
    immediate: bool,
    events: &broadcast::Sender<EventEnvelope>,
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
    let mut calls = Vec::new();
    if immediate {
        let mut query = database
            .connection
            .prepare("SELECT call,body FROM agent_questions WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        let mut rows = query
            .query([recipient.to_string()])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body: Vec<u8> = row.get(1).map_err(storage_error)?;
            let question: question::Question =
                serde_json::from_slice(&body).map_err(storage_error)?;
            if question.state == question::State::Pending {
                calls.push(row.get::<String>(0).map_err(storage_error)?);
            }
        }
    }
    if controls.guidance(recipient, request, immediate) {
        for call in calls {
            super::super::elicitation::cancel(database, recipient, &call, events).await?;
        }
    }
    Ok(())
}
