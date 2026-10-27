//! Durable guidance joins the active invocation at its next settled model boundary.
use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store) async fn recipient(
    db: &Connection,
    turn: TurnId,
    expected_revision: Option<u64>,
) -> Result<Run, Fault> {
    let run = runs::get(db, turn).await?;
    if run.kind != RunKind::Task || run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance requires an active task recipient",
        ));
    }
    if let Some(revision) = expected_revision {
        check_revision(run.revision, revision)?;
    }
    Ok(run)
}

pub(in crate::store) async fn pending(
    db: &Connection,
    recipient: TurnId,
) -> Result<Option<TurnId>, Fault> {
    { let mut statement = db.prepare("SELECT p.turn FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.recipient=?1 AND json_extract(a.body,'$.status')='queued' ORDER BY p.position LIMIT 1").await.map_err(storage_error)?;
match statement.query_row([recipient.to_string()]).await {
Ok(row) => { let mapped: turso::Result<_> = row.get::<String>(0); Some(mapped.map_err(storage_error)?) },
Err(turso::Error::QueryReturnedNoRows) => None,
Err(error) => return Err(storage_error(error)),
} }
    .map(|turn| turn.parse().map_err(storage_error))
    .transpose()
}

async fn same_capture(db: &Connection, source: &Run, target: &Run) -> Result<bool, Fault> {
    let provider = async |turn: TurnId| -> Result<Option<Vec<u8>>, Fault> {
        (async {
            let mut statement = db
                .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
                .await?;
            let row = statement.query_row([turn.to_string()]).await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .map_err(storage_error)
    };
    Ok(source.kind == RunKind::Task
        && source.session == target.session
        && source.worktree == target.worktree
        && super::super::permissions::config(db, source.turn).await?
            == super::super::permissions::config(db, target.turn).await?
        && super::super::super::sessions::roles::for_turn(db, source.turn).await?
            == super::super::super::sessions::roles::for_turn(db, target.turn).await?
        && super::super::plugins::read(db, source.turn).await?
            == super::super::plugins::read(db, target.turn).await?
        && super::super::super::hooks::frozen(db, source.turn).await?
            == super::super::super::hooks::frozen(db, target.turn).await?
        && provider(source.turn).await? == provider(target.turn).await?)
}

pub(in crate::store::agent) async fn compatible(
    db: &Connection,
    source: &Run,
    target: &Run,
) -> Result<(), Fault> {
    if !same_capture(db, source, target).await? {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance must retain the recipient's frozen configuration and capabilities",
        ));
    }
    Ok(())
}

pub(in crate::store) async fn tag(
    db: &Connection,
    source: TurnId,
    target: TurnId,
) -> Result<(), Fault> {
    let source = runs::get(db, source).await?;
    if source.status != Status::Queued {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance input is not queued",
        ));
    }
    let target = recipient(db, target, None).await?;
    compatible(db, &source, &target).await?;
    let items = read(db, source.session).await?.items;
    let mut others = items.iter().filter(|item| item.turn != source.turn);
    let before = if let Some(last) = items
        .iter()
        .rposition(|item| item.turn != source.turn && item.guidance == Some(target.turn))
    {
        items[last + 1..]
            .iter()
            .find(|item| item.turn != source.turn)
            .map(|item| item.turn)
    } else {
        others.next().map(|item| item.turn)
    };
    db.execute(
        "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE turn=?1",
        params![source.turn.to_string(), target.turn.to_string()],
    )
    .await
    .map_err(storage_error)?;
    runs::start(db, source.turn).await?;
    reorder(db, source.session, source.turn, before).await?;
    pause(db, source.session, false).await
}

pub(super) async fn select(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    source: TurnId,
    target: TurnId,
    expected_revision: u64,
) -> Result<(SessionId, Vec<Run>), Fault> {
    let source = check_item(db, source, expected_revision).await?;
    let session = super::super::super::commands::session(db, source.session).await?;
    super::super::super::sessions::writable(&session)?;
    let target = recipient(db, target, None).await?;
    if source.kind != RunKind::Task
        || source.session != target.session
        || source.worktree != target.worktree
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance belongs to another execution scope",
        ));
    }
    let recipients: std::collections::BTreeSet<_> = read(db, source.session)
        .await?
        .items
        .into_iter()
        .filter_map(|item| item.guidance)
        .collect();
    for recipient in recipients {
        let run = runs::get(db, recipient).await?;
        if !matches!(run.status, Status::Running | Status::Stopping) {
            clear(db, source.session, Some(recipient)).await?;
        }
    }
    let mut changed = Vec::new();
    let selected = if same_capture(db, &source, &target).await? {
        source.turn
    } else {
        // This explicit guidance command admits a new turn with the recipient's
        // capture. The original sent turn and its configuration remain immutable.
        let queued = message(db, source.turn).await?;
        let input: (String, Vec<u8>) = {
            let mut statement = db
                .prepare("SELECT request,caller FROM agent_pending WHERE turn=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([source.turn.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
            mapped.map_err(storage_error)?
        };
        let turn = inherit(db, caller, request, &target, &queued.message).await?;
        changed.push(runs::stop(db, source.turn).await?);
        // Keep the exact current message ledger reference, including prior edits.
        db.execute(
            "UPDATE agent_pending SET request=?2,caller=?3 WHERE turn=?1",
            params![turn.id.to_string(), input.0, input.1],
        )
        .await
        .map_err(storage_error)?;
        changed.push(runs::get(db, turn.id).await?);
        turn.id
    };
    runs::retry(db, selected).await?;
    tag(db, selected, target.turn).await?;
    if !changed.iter().any(|run| run.turn == selected) {
        changed.push(runs::get(db, selected).await?);
    }
    Ok((source.session, changed))
}

/// Admits input with the recipient's frozen configuration and capabilities.
pub(in crate::store) async fn inherit(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    target: &Run,
    message: &Input,
) -> Result<QueuedTurn, Fault> {
    super::validate(message)?;
    let mut session = super::super::super::commands::session(db, target.session).await?;
    session.config = super::super::permissions::config(db, target.turn).await?;
    session.roles = super::super::super::sessions::roles::for_turn(db, target.turn).await?;
    let plugins = super::super::plugins::read(db, target.turn).await?;
    super::super::references::validate(db, &session, message, Some(&plugins)).await?;
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind: RunKind::Task,
        session: target.session,
        request: request.id,
        revision: target.revision,
        config: session.config,
        roles: session.roles,
        plugins,
    };
    db.execute(
        "INSERT INTO turns(id,kind,session,revision,request,caller,plugins,roles,instructions,rowid) VALUES(?1,'task',?2,?3,?4,?5,?6,?7,?8,?9)",
        params![turn.id.to_string(), turn.session.to_string(), turn.revision as i64,
            request.id.to_string(), &caller.0[..], encode(&turn.plugins)?, encode(&turn.roles)?, encode(&super::super::super::captures::instructions(db, target.turn).await?)?, crate::store::database::counters::next(db, crate::store::database::counters::Namespace::Turns).await? as i64],
    ).await.map_err(storage_error)?;
    super::super::super::plugins::connectors::capture(db, turn.id, &turn.plugins).await?;
    super::super::super::attachments::turns::bind(db, turn.id, &message.attachments).await?;
    runs::inherit(db, &turn, target.turn, Status::Queued, true).await?;
    admit(db, &turn, true).await?;
    Ok(turn)
}

/// Main-session guidance admits and selects input in one controller command.
pub(in crate::store) async fn guide(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    target: Run,
) -> Result<(Output, Option<Event>), Fault> {
    let Command::GuideTurn { message, .. } = &request.command else {
        unreachable!()
    };
    if request.plugin.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "guidance requires a delegated child",
        ));
    }
    super::super::super::sessions::writable(
        &super::super::super::commands::session(db, target.session).await?,
    )?;
    let turn = inherit(db, caller, request, &target, message).await?;
    tag(db, turn.id, target.turn).await?;
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(super::super::activity::queued(db, turn).await?),
    ))
}

pub(in crate::store) async fn clear(
    db: &Connection,
    session: SessionId,
    recipient: Option<TurnId>,
) -> Result<(), Fault> {
    if db
        .execute(
            "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE recipient IS NOT NULL AND (?2 IS NULL OR recipient=?2) AND turn IN (SELECT id FROM turns WHERE session=?1)",
            params![session.to_string(), recipient.map(|turn| turn.to_string())],
        ).await
        .map_err(storage_error)?
        > 0
    {
        bump(db, session).await?;
    }
    Ok(())
}

pub(in crate::store) async fn recover(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT DISTINCT t.session FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient IS NOT NULL").await
        .map_err(storage_error)?;
    let sessions = (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = row.get::<String>(0);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    drop(query);
    for session in sessions {
        let session = session.parse().map_err(storage_error)?;
        pause(db, session, true).await?;
        clear(db, session, None).await?;
    }
    Ok(())
}

pub(in crate::store) async fn retain(
    database: &mut Database,
    target: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let run = runs::get(&transaction, target).await?;
        let child: bool = {
            let mut statement = transaction.prepare("SELECT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1)").await.map_err(storage_error)?;
            let row = statement
                .query_row([run.session.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        if child {
            super::super::delegation::retain_guidance(&transaction, Some(target)).await?;
        } else {
            pause(&transaction, run.session, true).await?;
            clear(&transaction, run.session, Some(target)).await?;
        }
        let queue = read(&transaction, run.session).await?;
        let envelope = super::super::changed(&transaction, database.node, run.session).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        publication.send(events, envelope);
        database
            .feeds
            .publish(database.node, run.session, Change::Queue(queue));
        Ok(())
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
