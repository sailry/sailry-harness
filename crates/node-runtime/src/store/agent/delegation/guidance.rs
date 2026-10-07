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
    let origin = origin(db, run.turn)?.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "guidance requires a delegated child",
        )
    })?;
    calls::active(db, origin.turn)?;
    if queue::guidance::pending(db, run.turn)?.is_some() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child already has pending guidance",
        ));
    }
    queue::validate(message)?;
    let mut session = super::super::super::commands::session(db, run.session)?;
    session.config = super::super::permissions::config(db, run.turn)?;
    session.roles = super::super::super::sessions::roles::read(db, run.session, run.revision)?;
    let plugins = super::super::plugins::read(db, run.turn)?;
    super::super::references::validate(db, &session, message, Some(&plugins))?;
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind: RunKind::Task,
        session: run.session,
        request: request.id,
        revision: run.revision,
        config: session.config,
        roles: session.roles,
        plugins,
    };
    db.execute(
        "INSERT INTO turns(id,kind,session,revision,request,caller,plugins) VALUES(?1,'task',?2,?3,?4,?5,?6)",
        params![turn.id.to_string(), turn.session.to_string(), turn.revision as i64,
            request.id.to_string(), &caller.0[..], encode(&turn.plugins)?],
    ).map_err(storage_error)?;
    super::super::super::attachments::turns::bind(db, turn.id, &message.attachments)?;
    runs::inherit(db, &turn, run.turn, Status::Queued, true)?;
    queue::admit(db, &turn, true)?;
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
