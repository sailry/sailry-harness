//! Composer metrics use the same pricing and timing as usage reports, scoped to visible history.
use super::*;

// Keep the pricing columns aligned with query::ROWS. Frozen providers belong to the original turn,
// including inherited history, not the viewing session's current configuration.
pub(in crate::store::agent::statistics) const ROWS: &str = "WITH selected AS (
    SELECT turn,0 AS delegated FROM conversation_turns WHERE session=?1
    UNION
    SELECT d.turn,1 FROM agent_delegations d
    JOIN conversation_turns h ON h.turn=d.parent_turn
    WHERE h.session=?1 AND NOT EXISTS(
        SELECT 1 FROM conversation_turns own WHERE own.session=?1 AND own.turn=d.turn)
    ), responses AS (SELECT e.body,coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) AS usage,h.delegated,
    CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END AS model,
    CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN a.provider ELSE json_extract(m.value,'$.provider') END AS provider
    FROM selected h JOIN agent_events e ON e.turn=h.turn
    JOIN turns t ON t.id=e.turn JOIN agent_runs a ON a.turn=t.id
    JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
    LEFT JOIN session_media media ON media.session=t.session
    LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.metadata.execution.media')
    WHERE coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'
    AND coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))>=0
    AND coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))>=0)
    SELECT NULL,
    max(0,json_extract(e.usage,'$.input')),
    max(0,json_extract(e.usage,'$.output')),
    max(0,coalesce(json_extract(e.usage,'$.cache_read'),0)),
    max(0,coalesce(json_extract(e.usage,'$.reasoning'),0)),
    NULL,e.model,NULL,NULL,
    json_extract(e.provider,'$.api'),json_extract(e.provider,'$.authentication'),
    json_extract(e.provider,'$.endpoint'),e.usage,
    json_extract(e.body,'$.metadata.execution.timing.elapsed_us'),
    json_extract(e.body,'$.metadata.execution.media'),e.delegated FROM responses e";

pub(in crate::store::agent::statistics) async fn read(
    db: &Connection,
    session: SessionId,
) -> Result<(Metrics, Metrics), Fault> {
    let mut own = Metrics::default();
    let mut delegated = Metrics::default();
    let mut prices = pricing::Catalog::default();
    let mut statement = db.prepare(ROWS).await.map_err(storage_error)?;
    let mut rows = statement
        .query([session.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let tokens = Usage {
            input: row.get::<i64>(1).map_err(storage_error)? as u64,
            output: row.get::<i64>(2).map_err(storage_error)? as u64,
            cached_input: row.get::<i64>(3).map_err(storage_error)? as u64,
            reasoning: row.get::<i64>(4).map_err(storage_error)? as u64,
        };
        let elapsed = row
            .get::<Option<i64>>(13)
            .map_err(storage_error)?
            .filter(|value| *value >= 0)
            .map(|value| value as u64);
        add(
            if row.get::<bool>(15).map_err(storage_error)? {
                &mut delegated
            } else {
                &mut own
            },
            &tokens,
            prices.estimate(db, &row).await?.as_ref(),
            elapsed,
        )?;
    }
    Ok((own, delegated))
}
