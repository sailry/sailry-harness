//! Composer metrics use the same pricing and timing as usage reports, scoped to visible history.
use super::*;

// Keep the pricing columns aligned with query::ROWS. Frozen providers belong to the original turn,
// including inherited history, not the viewing session's current configuration.
const ROWS: &str = "WITH responses AS (SELECT e.body,
    CASE WHEN json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
        THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END AS model,
    CASE WHEN json_extract(e.body,'$.event_metadata.sailry_media') IS NULL
        THEN a.provider ELSE json_extract(m.value,'$.provider') END AS provider
    FROM conversation_turns h JOIN agent_events e ON e.turn=h.turn
    JOIN turns t ON t.id=e.turn JOIN agent_runs a ON a.turn=t.id
    JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
    LEFT JOIN session_media media ON media.session=t.session
    LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.event_metadata.sailry_media')
    WHERE h.session=?1 AND json_type(e.body,'$.usage_metadata')='object')
    SELECT NULL,
    max(0,json_extract(e.body,'$.usage_metadata.prompt_token_count')),
    max(0,json_extract(e.body,'$.usage_metadata.candidates_token_count')),
    max(0,coalesce(json_extract(e.body,'$.usage_metadata.cache_read_input_token_count'),0)),
    max(0,coalesce(json_extract(e.body,'$.usage_metadata.thinking_token_count'),0)),
    NULL,e.model,NULL,NULL,
    json_extract(e.provider,'$.api'),json_extract(e.provider,'$.authentication'),
    json_extract(e.provider,'$.endpoint'),json_extract(e.body,'$.usage_metadata'),
    json_extract(e.body,'$.provider_metadata.sailry_timing.elapsed_us'),
    json_extract(e.body,'$.event_metadata.sailry_media') FROM responses e";

pub(in crate::store::agent::statistics) fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Metrics, Fault> {
    let mut metrics = Metrics::default();
    let mut prices = pricing::Catalog::default();
    let mut statement = db.prepare(ROWS).map_err(storage_error)?;
    let mut rows = statement
        .query([session.to_string()])
        .map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let tokens = Usage {
            input: row.get::<_, i64>(1).map_err(storage_error)? as u64,
            output: row.get::<_, i64>(2).map_err(storage_error)? as u64,
            cached_input: row.get::<_, i64>(3).map_err(storage_error)? as u64,
            reasoning: row.get::<_, i64>(4).map_err(storage_error)? as u64,
        };
        let elapsed = row
            .get::<_, Option<i64>>(13)
            .map_err(storage_error)?
            .filter(|value| *value > 0)
            .map(|value| value as u64);
        add(
            &mut metrics,
            &tokens,
            prices.estimate(db, row)?.as_ref(),
            elapsed,
        )?;
    }
    Ok(metrics)
}
