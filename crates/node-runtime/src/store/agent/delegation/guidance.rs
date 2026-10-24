use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store) async fn guide(
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
    let run = queue::guidance::recipient(db, *recipient, Some(*expected_revision)).await?;
    let Some(origin) = origin(db, run.turn).await? else {
        return queue::guidance::guide(db, caller, request, run).await;
    };
    if let Some(context) = &request.plugin {
        let parent = context.turn.ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "child guidance requires a parent turn",
            )
        })?;
        let owned = super::owned(db, parent, run.session).await?;
        if owned.turn != run.turn || context.session != Some(origin.session) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "guidance is outside the parent session",
            ));
        }
    } else {
        // A controller may observe a founding turn that has yielded to a newer
        // main-session turn. Canonical session ownership survives that handoff.
        let active: bool = {
            let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status')='running')").await.map_err(storage_error)?;
            let row = statement
                .query_row([origin.session.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
            mapped.map_err(storage_error)?
        };
        if !active {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "parent session is no longer running",
            ));
        }
    }
    if queue::guidance::pending(db, run.turn).await?.is_some() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child already has pending guidance",
        ));
    }
    let turn = queue::guidance::inherit(db, caller, request, &run, message).await?;
    follow(db, run.turn, turn.id).await?;
    queue::guidance::tag(db, turn.id, run.turn).await?;
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(activity::queued(db, turn).await?),
    ))
}

pub(in crate::store) async fn queued_guidance(
    db: &Connection,
    source: TurnId,
) -> Result<bool, Fault> {
    let Some(association) = origin(db, source).await? else {
        return Ok(false);
    };
    let body: Vec<u8> = {
        let mut statement = db.prepare("SELECT r.body FROM turns t JOIN requests r ON r.id=t.request AND r.caller=t.caller WHERE t.id=?1").await.map_err(storage_error)?;
        let row = statement
            .query_row([source.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let request: Request = serde_json::from_slice(&body).map_err(storage_error)?;
    let recipient = match request.command {
        Command::GuideTurn { recipient, .. } => Some(recipient),
        _ => None,
    };
    let Some(recipient) = recipient else {
        return Ok(false);
    };
    Ok(origin(db, recipient).await?.as_ref() == Some(&association)
        && runs::get(db, recipient).await?.session == runs::get(db, source).await?.session)
}

pub(in crate::store) async fn retain_guidance(
    db: &Connection,
    recipient: Option<TurnId>,
) -> Result<(), Fault> {
    if let Some(recipient) = recipient {
        if origin(db, recipient).await?.is_some() {
            let run = runs::get(db, recipient).await?;
            if !queue::read(db, run.session).await?.paused {
                queue::pause(db, run.session, true).await?;
            }
            queue::guidance::clear(db, run.session, Some(recipient)).await?;
        }
        return Ok(());
    }
    let mut query = db.prepare(
        "SELECT p.turn,t.session FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_delegations d ON d.turn=p.turn",
    ).await.map_err(storage_error)?;
    let pending = (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    drop(query);
    for (turn, session) in pending {
        let turn = turn.parse().map_err(storage_error)?;
        if !queued_guidance(db, turn).await? {
            continue;
        }
        let session = session.parse().map_err(storage_error)?;
        if !queue::read(db, session).await?.paused {
            queue::pause(db, session, true).await?;
        }
    }
    Ok(())
}
