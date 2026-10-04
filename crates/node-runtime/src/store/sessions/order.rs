//! Creation order with durable, project-scoped manual overrides in the Node event log.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::store) fn ordered(db: &Connection) -> Result<Vec<SessionId>, Fault> {
    let mut query = db
        .prepare("SELECT id FROM sessions WHERE state!='removed' ORDER BY rowid DESC")
        .map_err(storage_error)?;
    let mut sessions = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect::<Result<Vec<SessionId>, Fault>>()?;
    let saved: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM events WHERE json_extract(body,'$.kind')='sessions_reordered' ORDER BY cursor DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(saved) = saved {
        let Event::SessionsReordered(order) =
            serde_json::from_slice(&saved).map_err(storage_error)?
        else {
            return Err(commands::invalid("invalid saved session order"));
        };
        let positions: BTreeMap<_, _> = order
            .into_iter()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect();
        // Newly created sessions precede the saved order. Deleted sessions are absent.
        sessions.sort_by_key(|id| positions.get(id).copied());
    }
    Ok(sessions)
}

pub(in crate::store) fn reorder(
    db: &Connection,
    project: ProjectId,
    expected: &[SessionId],
    requested: &[SessionId],
) -> Result<(Output, Option<Event>), Fault> {
    let mut order = ordered(db)?;
    let mut query = db
        .prepare("SELECT id FROM sessions WHERE project=?1 AND state!='removed'")
        .map_err(storage_error)?;
    let members = query
        .query_map([project.to_string()], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect::<Result<BTreeSet<SessionId>, Fault>>()?;
    if members.is_empty() {
        return Err(Fault::new(ErrorCode::NotFound, "project has no sessions"));
    }
    if requested.len() != members.len()
        || requested.iter().copied().collect::<BTreeSet<_>>() != members
    {
        return Err(commands::invalid(
            "session order must contain each project session exactly once",
        ));
    }
    let current: Vec<_> = order
        .iter()
        .copied()
        .filter(|id| members.contains(id))
        .collect();
    if current != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "session order changed",
        ));
    }
    if current == requested {
        return Ok((Output::SessionOrder(order), None));
    }
    let mut requested = requested.iter();
    for id in &mut order {
        if members.contains(id) {
            *id = *requested.next().unwrap();
        }
    }
    Ok((
        Output::SessionOrder(order.clone()),
        Some(Event::SessionsReordered(order)),
    ))
}
