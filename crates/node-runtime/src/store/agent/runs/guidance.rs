//! A settled boundary claims all selected inputs in the same transaction.
use super::*;
use sailry_protocol::conversation::QueuedMessage;

pub(super) async fn claim(
    db: &Connection,
    primary: &Run,
    recipient: Option<TurnId>,
) -> Result<(Vec<QueuedMessage>, Vec<Run>), Fault> {
    let Some(recipient) = recipient else {
        return Ok((Vec::new(), Vec::new()));
    };
    let turns: Vec<_> = super::super::queue::read(db, primary.session)
        .await?
        .items
        .into_iter()
        .filter(|item| item.guidance == Some(recipient) && item.turn != primary.turn)
        .map(|item| item.turn)
        .collect();
    let mut inputs = Vec::with_capacity(turns.len());
    let mut joined = Vec::with_capacity(turns.len());
    for turn in turns {
        let mut run = get(db, turn).await?;
        super::super::queue::guidance::compatible(db, &run, primary).await?;
        let input = super::super::queue::message(db, turn).await?;
        capture(db, &input.turn, false, &input.message).await?;
        run.joined = Some(primary.turn);
        run.status = Status::Continued;
        run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        put(db, &run).await?;
        super::super::queue::remove(db, run.turn, run.session).await?;
        inputs.push(input);
        joined.push(run);
    }
    Ok((inputs, joined))
}
