//! Session visibility is durable Node state. Removed sessions retain immutable
//! history needed by forks and usage, but cannot be opened or resumed.
use super::*;
use crate::store::database::encode;

pub(in crate::store) async fn archive(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
    archived: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id).await?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    session.archived = archived;
    db.execute(
        "UPDATE sessions SET state=?2 WHERE id=?1",
        params![id.to_string(), if archived { "archived" } else { "active" }],
    )
    .await
    .map_err(storage_error)?;
    commands::revise_session(db, session).await
}

pub(in crate::store) async fn remove(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let session = commands::session(db, id).await?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    let ids = subtree(db, id).await?;
    for id in &ids {
        let busy: bool = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
             WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))").await.map_err(storage_error)?.query_row([id.to_string()]).await.and_then(|row| row.get(0)).map_err(storage_error)?;
        if busy {
            return Err(Fault::new(
                ErrorCode::Busy,
                "stop the session before deleting it",
            ));
        }
    }
    for id in &ids {
        crate::store::agent::background::dismiss(db, *id).await?;
        db.execute(
            "UPDATE sessions SET state='removed' WHERE id=?1",
            [id.to_string()],
        )
        .await
        .map_err(storage_error)?;
    }
    crate::store::artifacts::prune(db).await?;
    Ok((
        Output::SessionsRemoved(ids.clone()),
        Some(Event::SessionsRemoved(ids)),
    ))
}

/// Rename the same session title used by the agent naming tool.
pub(in crate::store) async fn rename(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
    title: &str,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id).await?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    let title = title.trim();
    if title.is_empty() || title.chars().any(char::is_control) {
        return Err(sailry_protocol::Fault::invalid(
            "session title must be nonempty and contain no control characters",
        ));
    }
    db.execute(
        "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body",
        params![id.to_string(), crate::store::agent::SESSION_TITLE, encode(&title)?],
    ).await
    .map_err(storage_error)?;
    session.activity.title = crate::store::agent::activity::title(db, id).await?;
    commands::revise_session(db, session).await
}

/// Use the caller's snapshot for every edge, preserving UNION's duplicate suppression.
async fn subtree(db: &Connection, root: SessionId) -> Result<Vec<SessionId>, Fault> {
    let mut ids = vec![root];
    let mut seen = std::collections::BTreeSet::from([root]);
    let mut index = 0;
    while index < ids.len() {
        let mut rows = db
            .query(
                "SELECT child.session FROM agent_delegations d \
             JOIN turns child ON child.id=d.turn \
             JOIN turns parent ON parent.id=d.parent_turn WHERE parent.session=?1",
                [ids[index].to_string()],
            )
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let child = row
                .get::<String>(0)
                .map_err(storage_error)?
                .parse::<SessionId>()
                .map_err(storage_error)?;
            if seen.insert(child) {
                ids.push(child);
            }
        }
        index += 1;
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
