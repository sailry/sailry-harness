use super::*;

mod compaction;
mod observations;
pub(in crate::store) mod report;

pub(super) async fn read(db: &Connection, session: SessionId) -> Result<Statistics, Fault> {
    check_session(db, session).await?;
    let turns = {
        let mut statement = db.prepare("SELECT count(*) FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND t.kind='task'
         AND EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=h.turn)").await.map_err(storage_error)?;
        statement
            .query_row([session.to_string()])
            .await
            .and_then(|row| row.get::<i64>(0))
            .map_err(storage_error)?
    };
    let (metrics, delegated) = report::session(db, session).await?;
    let observation = observation(db, session).await?;
    let context_tokens = observation.as_ref().and_then(|value| value.tokens);
    let context_limit = observation.as_ref().and_then(|value| value.context_limit);
    let context_estimate = if observation.as_ref().is_some_and(|value| value.summary) {
        context_usage(db, session).await?.map(|usage| {
            usage
                .tokens
                .saturating_add(crate::agent::estimate_context(&usage.pending))
        })
    } else {
        None
    };
    let context_model = observation.as_ref().and_then(|value| value.model.clone());
    let context_timestamp_ms = observation.as_ref().and_then(|value| value.timestamp_ms);
    let budget = compaction::budget(db, session).await?;
    let last_compaction = compaction::latest(db, session).await?;
    let first_token_us = observation.as_ref().and_then(|value| value.first_token_us);
    let observations = observations::read(db, session).await?;
    Ok(Statistics {
        observations,
        turns: turns as u64,
        responses: metrics.responses,
        usage: metrics.tokens,
        context_tokens,
        context_estimate,
        context_limit,
        context_model,
        context_timestamp_ms,
        budget,
        last_compaction,
        first_token_us,
        cost: metrics.cost,
        generation: metrics.generation,
        delegated,
    })
}

pub(super) async fn parents(db: &Connection, turn: TurnId) -> Result<Vec<SessionId>, Fault> {
    let mut query = db
        .prepare(
            "SELECT DISTINCT h.session FROM agent_delegations d
         JOIN conversation_turns h ON h.turn=d.parent_turn JOIN sessions s ON s.id=h.session
         WHERE d.turn=?1 AND s.state!='removed'",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query([turn.to_string()])
        .await
        .map_err(storage_error)?;
    let mut parents = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let session = row.get::<String>(0).map_err(storage_error)?;
        parents.push(session.parse().map_err(storage_error)?);
    }
    Ok(parents)
}

struct Observation {
    summary: bool,
    sequence: i64,
    tokens: Option<u64>,
    context_limit: Option<u32>,
    model: Option<String>,
    timestamp_ms: Option<i64>,
    first_token_us: Option<u64>,
}

pub(super) async fn context_usage(
    db: &Connection,
    session: SessionId,
) -> Result<Option<ContextUsage>, Fault> {
    let Some(observation) = observation(db, session).await? else {
        return Ok(None);
    };
    let (sequence, tokens, observed, summary) = match observation.tokens {
        Some(tokens) => (observation.sequence, tokens, true, None),
        None if observation.summary => {
            let Some((boundary, summary)) =
                super::compaction::read(db, session).await.map_err(|_| {
                    Fault::new(ErrorCode::Internal, "context summary could not be read")
                })?
            else {
                return Ok(None);
            };
            (boundary, 0, false, Some(summary))
        }
        None => return Ok(None),
    };
    // Both reads use the caller's transaction. Only new canonical content is decoded;
    // the observation already includes its model response's output tokens.
    let mut query = db
        .prepare(
            "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND h.sequence>?2
           AND json_type(e.body,'$.actions.compaction') IS NULL
           AND json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
         ORDER BY h.sequence",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), sequence])
        .await
        .map_err(storage_error)?;
    let mut pending = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
        pending.push(serde_json::from_slice(&body).map_err(storage_error)?);
    }
    if let Some(summary) = summary {
        pending.insert(0, summary);
    }
    Ok(Some(ContextUsage {
        observed,
        tokens,
        pending,
    }))
}

async fn observation(db: &Connection, session: SessionId) -> Result<Option<Observation>, Fault> {
    let latest = read_observation(db, session, false).await?;
    if latest
        .as_ref()
        .is_some_and(|value| value.tokens.is_none() && !value.summary)
        && let Some(reset) = read_observation(db, session, true).await?
        && reset.summary
    {
        // An unmetered response cannot replace a post-compaction estimate with
        // a real observation. Keep estimating the growing canonical window.
        return Ok(Some(reset));
    }
    Ok(latest)
}

async fn read_observation(
    db: &Connection,
    session: SessionId,
    measured: bool,
) -> Result<Option<Observation>, Fault> {
    // Read the last model observation in canonical order. A summary invalidates
    // the old window measurement; its own prompt is not the resumed context.
    let mut statement = db.prepare("SELECT h.sequence, CASE WHEN json_type(e.body,'$.actions.compaction') IS NULL
          AND json_extract(e.body,'$.usage_metadata.prompt_token_count') >= 0
          AND json_extract(e.body,'$.usage_metadata.candidates_token_count') >= 0
         THEN json_extract(e.body,'$.usage_metadata.prompt_token_count')
            + json_extract(e.body,'$.usage_metadata.candidates_token_count') END,
         CASE WHEN json_type(e.body,'$.actions.compaction') IS NULL
          AND json_extract(e.body,'$.usage_metadata.prompt_token_count') >= 0
          AND json_extract(e.body,'$.usage_metadata.candidates_token_count') >= 0
          AND json_type(e.body,'$.provider_metadata.sailry_timing.first_token_us')='integer'
          AND json_extract(e.body,'$.provider_metadata.sailry_timing.first_token_us') >= 0
         THEN json_extract(e.body,'$.provider_metadata.sailry_timing.first_token_us') END,
         (SELECT json_extract(model.value,'$.context') FROM json_each(r.provider,'$.models') model
          WHERE json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
         coalesce(json_type(e.body,'$.actions.compaction')='object',0),
         json_extract(revision.config,'$.model'),json_extract(e.body,'$.timestamp')
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         LEFT JOIN agent_runs r ON r.turn=e.turn LEFT JOIN turns t ON t.id=e.turn
         LEFT JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
         WHERE h.session=?1 AND json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
           AND (json_extract(e.body,'$.content.role')='model'
           OR json_type(e.body,'$.usage_metadata')='object'
           OR json_type(e.body,'$.actions.compaction')='object')
           AND (?2=0 OR json_type(e.body,'$.actions.compaction')='object'
             OR (json_extract(e.body,'$.usage_metadata.prompt_token_count')>=0
                 AND json_extract(e.body,'$.usage_metadata.candidates_token_count')>=0))
         ORDER BY h.sequence DESC LIMIT 1").await.map_err(storage_error)?;
    let row = match statement
        .query_row(params![session.to_string(), measured])
        .await
    {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let tokens = row
        .get::<Option<i64>>(1)
        .map_err(storage_error)?
        .map(|value| value as u64);
    let summary = row.get::<bool>(4).map_err(storage_error)?;
    let measured = tokens.is_some() || summary;
    Ok(Some(Observation {
        summary,
        sequence: row.get(0).map_err(storage_error)?,
        context_limit: if measured {
            row.get::<Option<u32>>(3)
                .map_err(storage_error)?
                .filter(|limit| *limit > 0)
        } else {
            None
        },
        model: if measured {
            row.get(5).map_err(storage_error)?
        } else {
            None
        },
        timestamp_ms: if measured {
            row.get::<Option<String>>(6)
                .map_err(storage_error)?
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
                .map(|value| value.timestamp_millis())
        } else {
            None
        },
        tokens,
        first_token_us: row
            .get::<Option<i64>>(2)
            .map_err(storage_error)?
            .map(|value| value as u64),
    }))
}

#[cfg(test)]
mod tests;
