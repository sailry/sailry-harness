use super::*;

// All event fields match agent_usage_time; joins read frozen configuration, not current defaults.
pub(super) const ROWS: &str = "WITH responses AS (SELECT e.body,coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) AS usage,e.sequence,e.session,e.turn,s.project,r.worktree,json_extract(r.config,'$.resource') AS resource,json_extract(r.config,'$.assistant') AS assistant,
    coalesce(json_extract(e.body,'$.metadata.execution.source.id'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN json_extract(r.config,'$.provider') ELSE json_extract(m.value,'$.provider.id') END) AS provider_id,
    coalesce(json_extract(e.body,'$.metadata.execution.source.model'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END) AS model,
    coalesce(json_extract(e.body,'$.metadata.execution.source'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN a.provider ELSE json_extract(m.value,'$.provider') END) AS provider
    FROM agent_events e JOIN turns t ON t.id=e.turn
    JOIN agent_runs a ON a.turn=t.id
    JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
    JOIN sessions s ON s.id=t.session
    LEFT JOIN session_media media ON media.session=t.session
    LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.metadata.execution.media')
    WHERE coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'
    AND coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))>=0
    AND coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))>=0
    AND substr(json_extract(e.body,'$.created'),1,19)>=?1
    AND substr(json_extract(e.body,'$.created'),1,19)<=?2
    AND (?3='[]' OR s.project IN (SELECT value FROM json_each(?3)))
    AND (?4='[]' OR r.worktree IN (SELECT value FROM json_each(?4))))
    SELECT
    json_extract(e.body,'$.created'),
    max(0,json_extract(e.usage,'$.input')),
    max(0,json_extract(e.usage,'$.output')),
    max(0,coalesce(json_extract(e.usage,'$.cache_read'),0)),
    max(0,coalesce(json_extract(e.usage,'$.reasoning'),0)),
    e.provider_id,e.model,e.project,e.worktree,
    json_extract(e.provider,'$.api'),json_extract(e.provider,'$.authentication'),
    json_extract(e.provider,'$.endpoint'),e.usage,
    json_extract(e.body,'$.metadata.execution.timing.elapsed_us'),
    json_extract(e.body,'$.metadata.execution.media'),e.sequence,e.session,e.turn,
    json_extract(e.body,'$.metadata.execution.timing.first_token_us'),
    json_extract(e.provider,'$.name'),coalesce(
        (SELECT name FROM projects WHERE id=e.project),
        CASE json_extract(e.resource,'$.kind')
            WHEN 'database' THEN (SELECT json_extract(body,'$.name') FROM database_profiles WHERE id=json_extract(e.resource,'$.id'))
            WHEN 'ssh' THEN (SELECT json_extract(body,'$.name') FROM ssh_profiles WHERE id=json_extract(e.resource,'$.id'))
        END, json_extract(e.resource,'$.id'),
        json_extract(e.assistant,'$.package.name') || ':' || json_extract(e.assistant,'$.id'),''),e.resource,e.assistant
    FROM responses e
    WHERE (?5='[]' OR e.provider_id IN (SELECT value FROM json_each(?5)))
    AND (?6='[]' OR e.model IN (SELECT value FROM json_each(?6)))
    ORDER BY substr(json_extract(e.body,'$.created'),1,19),e.sequence LIMIT ?7";

pub(super) fn validate(query: &Query) -> Result<(DateTime<Utc>, DateTime<Utc>), Fault> {
    if query.start_ms < 0
        || query.before.is_some_and(|before| {
            before.timestamp_ms < query.start_ms || before.timestamp_ms >= query.end_ms
        })
        || query.end_ms <= query.start_ms
        || query.end_ms - query.start_ms > 366 * DAY
        || query.utc_offset_minutes.unsigned_abs() > 14 * 60
        || [
            query.projects.len(),
            query.worktrees.len(),
            query.providers.len(),
            query.models.len(),
        ]
        .into_iter()
        .any(|length| length > 128)
        || query
            .models
            .iter()
            .any(|model| model.trim().is_empty() || model.len() > 256)
    {
        return Err(sailry_protocol::Fault::invalid(
            "usage time range or filters exceed the query limits",
        ));
    }
    let start = DateTime::from_timestamp_millis(query.start_ms)
        .ok_or_else(|| sailry_protocol::Fault::invalid("usage start time is invalid"))?;
    let end = DateTime::from_timestamp_millis(query.end_ms)
        .ok_or_else(|| sailry_protocol::Fault::invalid("usage end time is invalid"))?;
    Ok((start, end))
}

pub(super) fn id<T: serde::de::DeserializeOwned>(
    row: &turso::Row,
    index: usize,
) -> Result<T, Fault> {
    let value: String = row.get(index).map_err(storage_error)?;
    serde_json::from_value(serde_json::Value::String(value)).map_err(storage_error)
}
