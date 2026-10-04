//! Uses the command transaction for both history replacement and turn admission.
use super::*;

pub(in crate::store) fn replace(
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
    let current = commands::session(db, *session)?;
    commands::check_revision(current.revision, *expected_revision)?;
    let target = super::super::agent::visible_run(db, *session, *turn)?;
    if target.kind != conversation::RunKind::Task {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "only task turns can be edited",
        ));
    }
    let previous: Option<String> = db
        .query_row(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
             WHERE h.session=?1 AND t.rowid<?2 ORDER BY t.rowid DESC LIMIT 1",
            params![session.to_string(), target.sequence as i64],
            |row| row.get(0),
        )
        .optional()
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
    )?
    else {
        unreachable!("rewind result expected")
    };
    let turn = super::super::agent::queue::create(db, caller, request, current, message, true)?;
    let event = Event::SessionRewound {
        session: Box::new(commands::session(db, *session)?),
        backup: Box::new(history.backup.clone()),
    };
    Ok((Output::TurnReplaced { turn, history }, Some(event)))
}
