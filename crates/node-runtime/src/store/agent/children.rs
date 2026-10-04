//! Parent summaries are derived from child runs and frozen role metadata.
use super::*;
use sailry_protocol::conversation::Child as Summary;

#[cfg(test)]
mod tests;

pub(super) fn get(db: &Connection, run: &Run) -> Result<Option<Summary>, Fault> {
    delegation::read(db, run.session)?
        .map(|origin| project(db, run.clone(), *origin))
        .transpose()
}

pub(super) fn list(db: &Connection, entries: &[Entry]) -> Result<Vec<Summary>, Fault> {
    let calls: Vec<_> = entries
        .iter()
        .map(|entry| (entry.turn, &entry.id))
        .collect();
    let mut query = db
        .prepare(
            "SELECT a.body,d.body FROM session_delegations d JOIN turns t ON t.session=d.session
         JOIN agent_runs a ON a.turn=t.id
         JOIN json_each(?1) e ON d.parent_turn=json_extract(e.value,'$[0]')
             AND d.entry=json_extract(e.value,'$[1]')
         ORDER BY t.rowid",
        )
        .map_err(storage_error)?;
    query
        .query_map([encode(&calls)?], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (run, origin) = row.map_err(storage_error)?;
            project(
                db,
                serde_json::from_slice(&run).map_err(storage_error)?,
                serde_json::from_slice(&origin).map_err(storage_error)?,
            )
        })
        .collect()
}

fn project(db: &Connection, run: Run, origin: Delegation) -> Result<Summary, Fault> {
    let name = if let Some(role) = origin.role {
        let parent = runs::get(db, origin.turn)?;
        let roster = crate::store::sessions::roles::read(db, parent.session, parent.revision)?;
        Some(
            roster
                .profiles
                .into_iter()
                .find(|entry| entry.id == role)
                .ok_or_else(|| storage_error("frozen child role is missing"))?
                .name,
        )
    } else {
        None
    };
    Ok(Summary { run, origin, name })
}
