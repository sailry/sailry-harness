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
    let metrics = report::session(db, session)?;
    let context_tokens = context_tokens(db, session)?;
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
            cost: metrics.cost.clone(),
            generation: metrics.generation.clone(),
        })
    })
    .map_err(storage_error)
}

fn context_tokens(db: &Connection, session: SessionId) -> Result<Option<u64>, Fault> {
    observation(db, session).map(|value| value.map(|(_, tokens)| tokens))
}

pub(super) fn context_usage(
    db: &Connection,
    session: SessionId,
) -> Result<Option<ContextUsage>, Fault> {
    let Some((sequence, tokens)) = observation(db, session)? else {
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

fn observation(db: &Connection, session: SessionId) -> Result<Option<(i64, u64)>, Fault> {
    // Read the last model observation in canonical order. A summary invalidates
    // the old window measurement; its own prompt is not the resumed context.
    db.query_row(
        "SELECT h.sequence, CASE WHEN json_type(e.body,'$.actions.compaction') IS NULL
          AND json_extract(e.body,'$.usage_metadata.prompt_token_count') >= 0
          AND json_extract(e.body,'$.usage_metadata.candidates_token_count') >= 0
         THEN json_extract(e.body,'$.usage_metadata.prompt_token_count')
            + json_extract(e.body,'$.usage_metadata.candidates_token_count') END
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
           AND (json_extract(e.body,'$.content.role')='model'
           OR json_type(e.body,'$.usage_metadata')='object'
           OR json_type(e.body,'$.actions.compaction')='object')
         ORDER BY h.sequence DESC LIMIT 1",
        [session.to_string()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
    )
    .optional()
    .map(|value| {
        value.and_then(|(sequence, tokens)| tokens.map(|tokens| (sequence, tokens as u64)))
    })
    .map_err(storage_error)
}

#[cfg(test)]
mod tests;
