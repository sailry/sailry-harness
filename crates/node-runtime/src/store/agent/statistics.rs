use super::*;

pub(in crate::store) mod report;

// Expressions match the covering index; summaries do not materialize message bodies.
const TOTALS: &str = "SELECT count(*),
    coalesce(sum(max(0,json_extract(e.body,'$.usage_metadata.prompt_token_count'))),0),
    coalesce(sum(max(0,json_extract(e.body,'$.usage_metadata.candidates_token_count'))),0),
    coalesce(sum(max(0,coalesce(json_extract(e.body,'$.usage_metadata.cache_read_input_token_count'),0))),0),
    coalesce(sum(max(0,coalesce(json_extract(e.body,'$.usage_metadata.thinking_token_count'),0))),0)
    FROM conversation_turns h JOIN agent_events e ON e.turn=h.turn
    WHERE h.session=?1 AND json_type(e.body,'$.usage_metadata')='object'";

pub(super) fn read(db: &Connection, session: SessionId) -> Result<Statistics, Fault> {
    check_session(db, session)?;
    let turns = db
        .query_row(
            "SELECT count(*) FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND t.kind='task'
         AND EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=h.turn)",
            [session.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)?;
    let (metrics, delegated) = report::session(db, session)?;
    let observation = observation(db, session)?;
    let context_tokens = observation.as_ref().and_then(|value| value.tokens);
    let context_limit = observation.as_ref().and_then(|value| value.context_limit);
    let first_token_us = observation.as_ref().and_then(|value| value.first_token_us);
    db.query_row(TOTALS, [session.to_string()], |row| {
        let responses = row.get::<_, i64>(0)? as u64;
        let usage = if responses > 0 {
            Some(Usage {
                input: row.get::<_, i64>(1)? as u64,
                output: row.get::<_, i64>(2)? as u64,
                cached_input: row.get::<_, i64>(3)? as u64,
                reasoning: row.get::<_, i64>(4)? as u64,
            })
        } else {
            None
        };
        Ok(Statistics {
            turns: turns as u64,
            responses,
            usage,
            context_tokens,
            context_limit,
            first_token_us,
            cost: metrics.cost.clone(),
            generation: metrics.generation.clone(),
            delegated: delegated.clone(),
        })
    })
    .map_err(storage_error)
}

pub(super) fn parents(db: &Connection, turn: TurnId) -> Result<Vec<SessionId>, Fault> {
    let mut query = db
        .prepare(
            "SELECT DISTINCT h.session FROM agent_delegations d
         JOIN conversation_turns h ON h.turn=d.parent_turn JOIN sessions s ON s.id=h.session
         WHERE d.turn=?1 AND s.state!='removed'",
        )
        .map_err(storage_error)?;
    query
        .query_map([turn.to_string()], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect()
}

struct Observation {
    sequence: i64,
    tokens: Option<u64>,
    context_limit: Option<u32>,
    first_token_us: Option<u64>,
}

pub(super) fn context_usage(
    db: &Connection,
    session: SessionId,
) -> Result<Option<ContextUsage>, Fault> {
    let Some(Observation {
        sequence,
        tokens: Some(tokens),
        ..
    }) = observation(db, session)?
    else {
        return Ok(None);
    };
    // Both reads run in one storage job. Only new canonical content is decoded;
    // the observation already includes its model response's output tokens.
    let mut query = db
        .prepare(
            "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND h.sequence>?2
           AND json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
         ORDER BY h.sequence",
        )
        .map_err(storage_error)?;
    let pending = query
        .query_map(params![session.to_string(), sequence], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .map_err(storage_error)?
        .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(ContextUsage { tokens, pending }))
}

fn observation(db: &Connection, session: SessionId) -> Result<Option<Observation>, Fault> {
    // Read the last model observation in canonical order. A summary invalidates
    // the old window measurement; its own prompt is not the resumed context.
    db.query_row(
        "SELECT h.sequence, CASE WHEN json_type(e.body,'$.actions.compaction') IS NULL
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
          WHERE json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1)
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         LEFT JOIN agent_runs r ON r.turn=e.turn LEFT JOIN turns t ON t.id=e.turn
         LEFT JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
         WHERE h.session=?1 AND json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
           AND (json_extract(e.body,'$.content.role')='model'
           OR json_type(e.body,'$.usage_metadata')='object'
           OR json_type(e.body,'$.actions.compaction')='object')
         ORDER BY h.sequence DESC LIMIT 1",
        [session.to_string()],
        |row| {
            let tokens = row.get::<_, Option<i64>>(1)?.map(|value| value as u64);
            Ok(Observation {
                sequence: row.get(0)?,
                context_limit: if tokens.is_some() { row.get::<_, Option<u32>>(3)?.filter(|limit| *limit > 0) } else { None },
                tokens,
                first_token_us: row.get::<_, Option<i64>>(2)?.map(|value| value as u64),
            })
        },
    )
    .optional()
    .map_err(storage_error)
}

#[cfg(test)]
mod tests;
