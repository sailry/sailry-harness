use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store) fn guide(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let Command::GuideTurn {
        recipient,
        expected_revision,
        message,
    } = &request.command
    else {
        unreachable!()
    };
    let run = queue::guidance::recipient(db, *recipient, Some(*expected_revision))?;
    let Some(origin) = origin(db, run.turn)? else {
        return queue::guidance::guide(db, caller, request, run);
    };
    if let Some(context) = &request.plugin {
        let parent = context.turn.ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "child guidance requires a parent turn",
            )
        })?;
        let owned = super::owned(db, parent, run.session)?;
        if owned.turn != run.turn || context.session != Some(origin.session) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "guidance is outside the parent session",
            ));
        }
    } else {
        // A controller may observe a founding turn that has yielded to a newer
        // main-session turn. Canonical session ownership survives that handoff.
        let active: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status')='running')",
            [origin.session.to_string()], |row| row.get(0),
        ).map_err(storage_error)?;
        if !active {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "parent session is no longer running",
            ));
        }
    }
    if queue::guidance::pending(db, run.turn)?.is_some() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child already has pending guidance",
        ));
    }
    let turn = queue::guidance::inherit(db, caller, request, &run, message)?;
    follow(db, run.turn, turn.id)?;
    queue::guidance::tag(db, turn.id, run.turn)?;
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(activity::queued(db, turn)?),
    ))
}

pub(in crate::store) fn queued_guidance(db: &Connection, source: TurnId) -> Result<bool, Fault> {
    let Some(association) = origin(db, source)? else {
        return Ok(false);
    };
    let body: Vec<u8> = db.query_row(
        "SELECT r.body FROM turns t JOIN requests r ON r.id=t.request AND r.caller=t.caller WHERE t.id=?1",
        [source.to_string()], |row| row.get(0),
    ).map_err(storage_error)?;
    let request: Request = serde_json::from_slice(&body).map_err(storage_error)?;
    let recipient = match request.command {
        Command::GuideTurn { recipient, .. } => Some(recipient),
        _ => None,
    };
    let Some(recipient) = recipient else {
        return Ok(false);
    };
    Ok(origin(db, recipient)?.as_ref() == Some(&association)
        && runs::get(db, recipient)?.session == runs::get(db, source)?.session)
}

pub(in crate::store) fn retain_guidance(
    db: &Connection,
    recipient: Option<TurnId>,
) -> Result<(), Fault> {
    if let Some(recipient) = recipient {
        if origin(db, recipient)?.is_some() {
            let run = runs::get(db, recipient)?;
            if !queue::read(db, run.session)?.paused {
                queue::pause(db, run.session, true)?;
            }
            queue::guidance::clear(db, run.session, Some(recipient))?;
        }
        return Ok(());
    }
    let mut query = db.prepare(
        "SELECT p.turn,t.session FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_delegations d ON d.turn=p.turn",
    ).map_err(storage_error)?;
    let pending = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(query);
    for (turn, session) in pending {
        let turn = turn.parse().map_err(storage_error)?;
        if !queued_guidance(db, turn)? {
            continue;
        }
        let session = session.parse().map_err(storage_error)?;
        if !queue::read(db, session)?.paused {
            queue::pause(db, session, true)?;
        }
    }
    Ok(())
}
