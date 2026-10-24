//! Parent summaries are derived from child runs and frozen role metadata.
use super::*;
use sailry_protocol::conversation::Child as Summary;

#[cfg(test)]
mod tests;

pub(super) async fn get(db: &Connection, run: &Run) -> Result<Option<Summary>, Fault> {
    match delegation::origin(db, run.turn).await? {
        Some(origin) => Ok(Some(project(db, run.clone(), origin).await?)),
        None => Ok(None),
    }
}

pub(super) async fn list(db: &Connection, entries: &[Entry]) -> Result<Vec<Summary>, Fault> {
    let calls: Vec<_> = entries
        .iter()
        .map(|entry| (entry.turn, &entry.id))
        .collect();
    let mut rows = db.query("SELECT a.body,d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn JOIN agent_runs a ON a.turn=d.turn JOIN json_each(?1) e ON d.parent_turn=json_extract(e.value,'$[0]') AND d.entry=json_extract(e.value,'$[1]') ORDER BY t.rowid", [encode(&calls)?]).await.map_err(storage_error)?;
    let mut selected = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let run: Vec<u8> = row.get(0).map_err(storage_error)?;
        let origin: Vec<u8> = row.get(1).map_err(storage_error)?;
        selected.push(
            project(
                db,
                serde_json::from_slice(&run).map_err(storage_error)?,
                serde_json::from_slice(&origin).map_err(storage_error)?,
            )
            .await?,
        );
    }
    Ok(selected)
}

async fn project(db: &Connection, run: Run, origin: Delegation) -> Result<Summary, Fault> {
    let name = delegation::child(db, run.turn)
        .await?
        .ok_or_else(|| storage_error("child summary has no delegation"))?
        .role
        .map(|role| role.name);
    Ok(Summary { run, origin, name })
}
