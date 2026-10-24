//! Package-owned automatic input uses the ordinary queue and an opaque KV revision fence.
use super::*;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Fence {
    package: plugin::Reference,
    key: String,
    revision: u64,
    scope: plugin::storage::Scope,
}

pub(in crate::store) async fn admit(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let Command::ContinueTurn {
        session,
        after,
        message,
        key,
        expected_revision,
        scope,
    } = &request.command
    else {
        unreachable!()
    };
    let context = request.plugin.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "continuation requires plugin provenance",
        )
    })?;
    let fence = Fence {
        package: context.package.clone(),
        key: key.clone(),
        revision: *expected_revision,
        scope: *scope,
    };
    if !valid(db, &fence, *session).await? {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "continuation data changed",
        ));
    }
    let session = super::super::commands::session(db, *session).await?;
    let child: bool = {
        let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1)").await.map_err(storage_error)?;
        let row = statement
            .query_row([session.id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if session.config.mode != WorkMode::Code || child {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "session does not accept automatic input",
        ));
    }
    let queue = queue::read(db, session.id).await?;
    if queue.paused
        || !queue.items.is_empty()
        || runs::list(db, session.id, &[])
            .await?
            .iter()
            .any(|run| matches!(run.status, Status::Running | Status::Stopping))
    {
        return Err(Fault::new(ErrorCode::Busy, "session has pending input"));
    }
    if let Some(after) = after {
        let run = runs::visible(db, session.id, *after).await?;
        let latest: Option<String> = {
            let mut statement = db.prepare("SELECT t.id FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.session=?1 ORDER BY t.rowid DESC LIMIT 1").await.map_err(storage_error)?;
            match statement.query_row([session.id.to_string()]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        if run.origin.is_some()
            || run.status != Status::Completed
            || latest.as_deref() != Some(after.to_string().as_str())
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "continuation source is no longer current",
            ));
        }
    }
    let turn = queue::create(db, caller, request, session, message, true).await?;
    db.execute(
        "INSERT INTO agent_continuations(turn,body) VALUES(?1,?2)",
        params![turn.id.to_string(), encode(&fence)?],
    )
    .await
    .map_err(storage_error)?;
    let package = super::plugins::packages(db, std::slice::from_ref(&context.package))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| Fault::invalid("continuation package is unavailable"))?;
    let mut run = runs::get(db, turn.id).await?;
    run.continuation = Some(sailry_protocol::conversation::Continuation {
        package: context.package.clone(),
        marker: package
            .extension
            .and_then(|extension| extension.continuation_marker),
    });
    runs::put(db, &run).await?;
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(activity::queued(db, turn).await?),
    ))
}

async fn valid(db: &Connection, fence: &Fence, session: SessionId) -> Result<bool, Fault> {
    let package = super::super::plugins::get(db, &fence.package.name).await?;
    if !package.is_some_and(|package| {
        package.summary.enabled && package.summary.reference() == fence.package
    }) {
        return Ok(false);
    }
    match fence.scope {
        plugin::storage::Scope::Node => {
            let value =
                super::super::plugins::storage::read(db, &fence.package.name, &fence.key).await?;
            Ok(value.present && value.revision == fence.revision)
        }
        plugin::storage::Scope::Conversation => {
            let value = super::super::plugins::storage::conversation::read(
                db,
                &fence.package.name,
                session,
                &fence.key,
            )
            .await?;
            Ok(value.present && !value.restored && value.revision == fence.revision)
        }
    }
}

pub(super) async fn check(db: &Connection, run: &Run) -> Result<Option<Fault>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_continuations WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([run.turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some(body) = body else { return Ok(None) };
    let fence: Fence = serde_json::from_slice(&body).map_err(storage_error)?;
    let session = super::super::commands::session(db, run.session).await?;
    let pending: bool = {
        let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1 AND p.turn!=?2 AND NOT EXISTS(SELECT 1 FROM agent_continuations c WHERE c.turn=p.turn))").await.map_err(storage_error)?;
        let row = statement
            .query_row(params![run.session.to_string(), run.turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if pending {
        return Ok(Some(Fault::new(
            ErrorCode::Conflict,
            "automatic input was superseded by user input",
        )));
    }
    Ok(
        (session.config.mode != WorkMode::Code || !valid(db, &fence, run.session).await?).then(
            || {
                Fault::new(
                    ErrorCode::RevisionConflict,
                    "automatic input is no longer available",
                )
            },
        ),
    )
}

pub(super) async fn recover(db: &Connection) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT c.turn FROM agent_continuations c JOIN agent_runs a ON a.turn=c.turn WHERE json_extract(a.body,'$.status')='queued'").await.map_err(storage_error)?;
    let turns = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<String>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    drop(query);
    for turn in turns {
        runs::stop(db, turn.parse().map_err(storage_error)?).await?;
    }
    Ok(())
}

pub(in crate::store) async fn invalidate(db: &Connection, name: &str) -> Result<(), Fault> {
    let mut query=db.prepare("SELECT c.turn,c.body FROM agent_continuations c JOIN agent_runs a ON a.turn=c.turn WHERE json_extract(a.body,'$.status') IN ('queued','running')").await.map_err(storage_error)?;
    let rows = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    drop(query);
    for (turn, body) in rows {
        let fence: Fence = serde_json::from_slice(&body).map_err(storage_error)?;
        if fence.package.name == name {
            runs::stop(db, turn.parse().map_err(storage_error)?).await?;
        }
    }
    Ok(())
}

pub(in crate::store) async fn stopping(db: &Connection) -> Result<Vec<TurnId>, Fault> {
    let mut query=db.prepare("SELECT c.turn FROM agent_continuations c JOIN agent_runs a ON a.turn=c.turn WHERE json_extract(a.body,'$.status')='stopping'").await.map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<String>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
    .collect()
}

pub(in crate::store) async fn sessions(
    db: &Connection,
    name: &str,
) -> Result<Vec<SessionId>, Fault> {
    let mut query=db.prepare("SELECT DISTINCT t.session FROM agent_continuations c JOIN turns t ON t.id=c.turn WHERE json_extract(c.body,'$.package.name')=?1").await.map_err(storage_error)?;
    {
        let mut rows = query.query([name]).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<String>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
    .collect()
}
