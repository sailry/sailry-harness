//! Policy and maintenance references come from Node state, never a loaded history page.
use super::*;
use sailry_protocol::conversation::compaction::{Budget, Failure, Outcome, Record};

pub(super) async fn budget(db: &Connection, session: SessionId) -> Result<Option<Budget>, Fault> {
    let captured: Option<(Option<Vec<u8>>, Vec<u8>)> = {
        let mut statement = db.prepare("SELECT r.provider,revision.config FROM agent_runs r JOIN turns t ON t.id=r.turn
         JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
         LEFT JOIN agent_pending pending ON pending.turn=r.turn
         WHERE t.session=?1 AND json_extract(r.body,'$.status') IN ('running','stopping','queued')
         ORDER BY CASE WHEN json_extract(r.body,'$.status')='queued' THEN 1 ELSE 0 END,
           pending.position,t.rowid LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => Some((
                row.get(0).map_err(storage_error)?,
                row.get(1).map_err(storage_error)?,
            )),
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (provider, config) = if let Some((provider, config)) = captured {
        let provider: Option<Provider> = provider
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(storage_error)?;
        let config: SessionConfig = serde_json::from_slice(&config).map_err(storage_error)?;
        (provider, config)
    } else {
        let current = crate::store::commands::session(db, session).await?;
        let provider = match current.profile {
            Some(profile) => Some(profile.provider),
            None => providers::get(db, current.config.provider).await?,
        };
        (provider, current.config)
    };
    Ok(provider.and_then(|provider| {
        provider
            .models
            .iter()
            .find(|model| model.id == config.model)
            .and_then(|model| {
                crate::agent::compaction_budget(
                    &model.id,
                    model.context,
                    model.output,
                    model.compaction_tokens,
                )
            })
    }))
}

pub(super) async fn latest(db: &Connection, session: SessionId) -> Result<Option<Record>, Fault> {
    let row: Option<(i64, String, Vec<u8>, i64)> = {
        let mut statement = db.prepare("SELECT h.sequence,e.turn,e.body,t.rowid FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence JOIN turns t ON t.id=e.turn WHERE h.session=?1 AND
         (json_type(e.body,'$.actions.compaction')='object'
          OR json_type(e.body,'$.event_metadata.sailry_compaction_failure')='text'
          OR json_extract(e.body,'$.event_metadata.sailry_operation_warning')='context_summary_failed')
         ORDER BY h.sequence DESC LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => Some((
                row.get(0).map_err(storage_error)?,
                row.get(1).map_err(storage_error)?,
                row.get(2).map_err(storage_error)?,
                row.get(3).map_err(storage_error)?,
            )),
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some((sequence, turn, body, turn_sequence)) = row else {
        return Ok(None);
    };
    let sequence = sequence.try_into().map_err(storage_error)?;
    let turn_sequence = turn_sequence.try_into().map_err(storage_error)?;
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    let turn = turn.parse().map_err(storage_error)?;
    let outcome = if event.actions.compaction.is_some() {
        Outcome::Succeeded
    } else {
        let entry = super::super::history::project(db, sequence, turn, &event).await?;
        let failure = entry
            .parts
            .iter()
            .find_map(|part| match part {
                Part::Resource(value) => Failure::from_resource(value),
                _ => None,
            })
            .ok_or_else(|| Fault::invalid("compaction record is missing its failure"))?;
        Outcome::Failed(failure)
    };
    Ok(Some(Record {
        id: event.id,
        sequence,
        turn,
        turn_sequence,
        timestamp_ms: event.timestamp.timestamp_millis(),
        outcome,
    }))
}
