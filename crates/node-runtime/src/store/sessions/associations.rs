//! Immutable business links share the existing session store and scoped commands.
use super::*;
use conversation::{AssociatedSessions, Association};

pub(in crate::store) async fn bind(
    db: &Connection,
    session: SessionId,
    association: &Association,
) -> Result<(), Fault> {
    association.validate()?;
    db.execute(
        "INSERT INTO session_associations(session,namespace,kind,identity) VALUES(?1,?2,?3,?4)",
        params![
            session.to_string(),
            association.namespace.as_str(),
            association.kind.as_str(),
            association.id.as_str()
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Option<Association>, Fault> {
    let mut query = db
        .prepare("SELECT namespace,kind,identity FROM session_associations WHERE session=?1")
        .await
        .map_err(storage_error)?;
    match query.query_row([session.to_string()]).await {
        Ok(row) => Ok(Some(Association {
            namespace: row.get(0).map_err(storage_error)?,
            kind: row.get(1).map_err(storage_error)?,
            id: row.get(2).map_err(storage_error)?,
        })),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

pub(in crate::store) async fn list(
    db: &Connection,
    request: &Request,
    association: &Association,
    before: Option<SessionId>,
    limit: u16,
) -> Result<AssociatedSessions, Fault> {
    association.validate()?;
    if !(1..=100).contains(&limit) {
        return Err(Fault::invalid(
            "session page limit must be between 1 and 100",
        ));
    }
    let worktree = request.plugin.as_ref().and_then(|context| context.worktree);
    let sequence = match before {
        None => i64::MAX,
        Some(id) => {
            let session = commands::session(db, id).await?;
            if session.association.as_ref() != Some(association)
                || worktree.is_some_and(|tree| tree != session.worktree)
            {
                return Err(Fault::new(
                    ErrorCode::WrongTarget,
                    "session cursor belongs to another association",
                ));
            }
            session.creation_order.try_into().map_err(storage_error)?
        }
    };
    let mut query = db
        .prepare(
            "SELECT s.id FROM sessions s JOIN session_associations a ON a.session=s.id
         WHERE a.namespace=?1 AND a.kind=?2 AND a.identity=?3 AND s.state!='removed'
         AND s.rowid<?4 AND (?5 IS NULL OR s.worktree=?5) ORDER BY s.rowid DESC LIMIT ?6",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![
            association.namespace.as_str(),
            association.kind.as_str(),
            association.id.as_str(),
            sequence,
            worktree.map(|tree| tree.to_string()),
            i64::from(limit) + 1
        ])
        .await
        .map_err(storage_error)?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let id: String = row.get(0).map_err(storage_error)?;
        ids.push(id.parse::<SessionId>().map_err(storage_error)?);
    }
    let more = ids.len() > usize::from(limit);
    ids.truncate(usize::from(limit));
    let next_before = more.then(|| *ids.last().unwrap());
    let mut sessions = Vec::with_capacity(ids.len());
    for id in ids {
        sessions.push(commands::session(db, id).await?);
    }
    Ok(AssociatedSessions {
        sessions,
        next_before,
    })
}

/// An unscoped plugin may address only sessions it explicitly associated at creation.
pub(in crate::store) async fn owns(
    db: &Connection,
    context: &plugin::Context,
    session: SessionId,
) -> Result<bool, Fault> {
    if context.turn.is_some() || context.session.is_some() {
        return Ok(false);
    }
    let session = commands::session(db, session).await?;
    Ok(session.delegation.is_none()
        && context.worktree.is_none_or(|tree| tree == session.worktree)
        && session
            .association
            .as_ref()
            .is_some_and(|association| association.namespace == context.package.name))
}
