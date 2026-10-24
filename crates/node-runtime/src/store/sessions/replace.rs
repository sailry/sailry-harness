//! Uses the command transaction for both history replacement and turn admission.
use super::*;

pub(in crate::store) async fn replace(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let Command::ReplaceTurn {
        session,
        turn,
        expected_head,
        expected_history_revision,
        expected_revision,
        message,
    } = &request.command
    else {
        unreachable!("replacement command expected")
    };
    let current = commands::session(db, *session).await?;
    commands::check_revision(current.revision, *expected_revision)?;
    let target = super::super::agent::visible_run(db, *session, *turn).await?;
    if target.kind != conversation::RunKind::Task {
        return Err(Fault::invalid("only task turns can be edited"));
    }
    let previous: Option<String> = match db
        .prepare(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
             WHERE h.session=?1 AND t.rowid<?2 ORDER BY t.rowid DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row(params![session.to_string(), target.sequence as i64])
        .await
        .and_then(|row| row.get(0))
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
    .map_err(storage_error)?;
    let through = previous
        .map(|id| id.parse())
        .transpose()
        .map_err(storage_error)?;
    let (Output::Rewound(history), _) = rewind(
        db,
        *session,
        through,
        *expected_head,
        *expected_history_revision,
        true,
    )
    .await?
    else {
        unreachable!("rewind result expected")
    };
    let turn =
        super::super::agent::queue::create(db, caller, request, current, message, true).await?;
    let event = Event::SessionRewound {
        session: Box::new(commands::session(db, *session).await?),
        backup: history.backup.clone().map(Box::new),
    };
    Ok((Output::TurnReplaced { turn, history }, Some(event)))
}
