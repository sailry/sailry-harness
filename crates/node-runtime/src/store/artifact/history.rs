//! Forks retain exact published receipts without acquiring their write ownership.
use super::*;

pub(super) async fn published(
    db: &Connection,
    session: SessionId,
    artifact: &Summary,
) -> Result<bool, Fault> {
    let mut query = db.prepare("SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2").await.map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), artifact.turn.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let event: adk_core::Event =
            serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?;
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            let adk_core::Part::FunctionResponse {
                function_response, ..
            } = part
            else {
                continue;
            };
            if tool::Presentation::Content
                .content(&function_response.response)
                .is_some_and(|content| {
                    content.blocks.iter().any(
                        |block| matches!(block, tool::Block::Artifact { artifact: value } if value == artifact),
                    )
                })
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn retained(db: &Connection, artifact: &Summary) -> Result<bool, Fault> {
    let mut query = db.prepare("SELECT DISTINCT h.session FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence JOIN sessions s ON s.id=h.session WHERE e.turn=?1 AND s.state!='removed'").await.map_err(storage_error)?;
    let mut rows = query
        .query([artifact.turn.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let session: SessionId = row
            .get::<String>(0)
            .map_err(storage_error)?
            .parse()
            .map_err(storage_error)?;
        if published(db, session, artifact).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn referenced(db: &Connection, id: &str) -> Result<bool, Fault> {
    let mut query = db
        .prepare("SELECT body FROM artifact_revisions WHERE artifact=?1")
        .await
        .map_err(storage_error)?;
    let mut rows = query.query([id]).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let artifact: Summary =
            serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?;
        if retained(db, &artifact).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in crate::store) async fn prune(db: &Connection) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT a.id FROM artifacts a JOIN sessions s ON s.id=a.session WHERE s.state='removed'").await.map_err(storage_error)?;
    let ids = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    for id in ids {
        if !referenced(db, &id).await? {
            db.execute("DELETE FROM artifacts WHERE id=?1", [id.as_str()])
                .await
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

pub(in crate::store) async fn cleanup(db: &Connection) -> Result<Vec<SessionId>, Fault> {
    let mut query = db.prepare("SELECT s.id FROM sessions s WHERE s.state='removed' AND NOT EXISTS(SELECT 1 FROM artifacts a WHERE a.session=s.id)").await.map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
    .collect()
}

pub(super) async fn validate(db: &Connection) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT a.id FROM artifacts a JOIN sessions s ON s.id=a.session WHERE s.state='removed'").await.map_err(storage_error)?;
    let mut rows = query.query(()).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let id: String = row.get(0).map_err(storage_error)?;
        if !referenced(db, &id).await? {
            return Err(storage_error(
                "removed artifact owner has no retained publication",
            ));
        }
    }
    Ok(())
}
