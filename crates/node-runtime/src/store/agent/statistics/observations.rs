//! Read lightweight observations through the canonical conversation membership.
use super::*;
use sailry_protocol::conversation::observation::{Category, RECENT_LIMIT, Request, Summary};
use std::collections::BTreeMap;

pub(super) fn read(db: &Connection, session: SessionId) -> Result<Summary, Fault> {
    let mut result = Summary::default();
    let mut calls = BTreeMap::<String, (u64, Request)>::new();
    let mut composition = BTreeMap::new();
    let mut query = db.prepare(
        "SELECT h.sequence,json_extract(e.body,'$.event_metadata.sailry_model_observation'),
          (SELECT json_extract(model.value,'$.context') FROM agent_runs r
           JOIN turns t ON t.id=r.turn
           JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision,
           json_each(r.provider,'$.models') model WHERE r.turn=e.turn
           AND json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1)
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND json_type(e.body,'$.event_metadata.sailry_model_observation')='text'
         ORDER BY h.sequence"
    ).map_err(storage_error)?;
    let mut rows = query.query([session.to_string()]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let sequence =
            u64::try_from(row.get::<_, i64>(0).map_err(storage_error)?).map_err(storage_error)?;
        let encoded = row.get::<_, String>(1).map_err(storage_error)?;
        let mut request: Request = serde_json::from_str(&encoded).map_err(storage_error)?;
        request.context_limit = row.get(2).map_err(storage_error)?;
        calls.insert(request.id.clone(), (sequence, request));
    }
    let scalar = |expression: &str| -> Result<u64, Fault> {
        let value: i64 = db.query_row(&format!(
            "SELECT {expression} FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1"),
            [session.to_string()], |row| row.get(0)).map_err(storage_error)?;
        u64::try_from(value).map_err(storage_error)
    };
    result.compactions =
        scalar("count(CASE WHEN json_type(e.body,'$.actions.compaction')='object' THEN 1 END)")?;
    result.replies = scalar(
        "count(DISTINCT CASE WHEN json_extract(e.body,'$.content.role')='model' AND EXISTS (SELECT 1 FROM json_each(e.body,'$.content.parts') p WHERE length(json_extract(p.value,'$.text'))>0) THEN e.turn END)",
    )?;
    let usage_field = |field: &str| -> Result<Option<u64>, Fault> {
        let value: Option<i64> = db.query_row(&format!(
            "SELECT CASE WHEN count(*)>0 AND count(*)=count(json_extract(e.body,'$.usage_metadata.{field}')) THEN sum(max(0,json_extract(e.body,'$.usage_metadata.{field}'))) END
             FROM conversation_turns h JOIN agent_events e ON e.turn=h.turn WHERE h.session=?1 AND json_type(e.body,'$.usage_metadata')='object'"),
            [session.to_string()], |row| row.get(0)).map_err(storage_error)?;
        value.map(u64::try_from).transpose().map_err(storage_error)
    };
    result.cache_read = usage_field("cache_read_input_token_count")?;
    result.reasoning = usage_field("thinking_token_count")?;
    let mut tools = db.prepare(
        "SELECT json_extract(p.value,'$.name'),count(*)
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence, json_each(e.body,'$.content.parts') p
         WHERE h.session=?1 AND json_type(p.value,'$.name')='text' AND json_type(p.value,'$.args')='object'
         GROUP BY json_extract(p.value,'$.name')"
    ).map_err(storage_error)?;
    result.tools = tools
        .query_map([session.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (name, count) = row.map_err(storage_error)?;
            Ok((name, u64::try_from(count).map_err(storage_error)?))
        })
        .collect::<Result<_, Fault>>()?;
    let mut phases = db.prepare(
        "SELECT json_extract(e.body,'$.event_metadata.sailry_execution_phases') FROM conversation_turns h
         JOIN agent_events e ON e.turn=h.turn WHERE h.session=?1
         AND e.sequence=(SELECT max(latest.sequence) FROM agent_events latest WHERE latest.turn=e.turn
         AND json_type(latest.body,'$.event_metadata.sailry_execution_phases')='text')"
    ).map_err(storage_error)?;
    let mut rows = phases.query([session.to_string()]).map_err(storage_error)?;
    let mut recorded = 0u64;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let value: sailry_protocol::conversation::observation::Phases =
            serde_json::from_str(&row.get::<_, String>(0).map_err(storage_error)?)
                .map_err(storage_error)?;
        if recorded == 0 {
            result.phases = value;
        } else {
            for (sum, next) in [
                (&mut result.phases.active_us, value.active_us),
                (&mut result.phases.waiting_us, value.waiting_us),
                (&mut result.phases.output_us, value.output_us),
                (&mut result.phases.tools_us, value.tools_us),
                (&mut result.phases.other_us, value.other_us),
            ] {
                *sum = sum
                    .zip(next)
                    .map(|(left, right)| left.saturating_add(right));
            }
        }
        recorded += 1;
    }
    let missing: i64 = db.query_row(
        "SELECT count(*) FROM conversation_turns h JOIN agent_runs r ON r.turn=h.turn WHERE h.session=?1 AND json_extract(r.body,'$.status') IN ('completed','failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=h.turn AND json_type(e.body,'$.event_metadata.sailry_execution_phases')='text')",
        [session.to_string()], |row| row.get(0)).map_err(storage_error)?;
    if u64::try_from(missing).map_err(storage_error)? > 0 {
        result.phases = Default::default();
    }
    result.calls = calls.len() as u64;
    let children: i64 = db.query_row(
        "SELECT count(DISTINCT d.turn) FROM agent_delegations d JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1",
        [session.to_string()], |row| row.get(0)
    ).map_err(storage_error)?;
    result.children = u64::try_from(children).map_err(storage_error)?;
    let mut calls = calls.into_values().collect::<Vec<_>>();
    calls.sort_by_key(|(sequence, _)| *sequence);
    for (_, call) in &calls {
        for attempt in &call.attempts {
            result.model_us = result
                .model_us
                .saturating_add(attempt.elapsed_us.unwrap_or_default());
            if attempt.reasoning_reported
                && let Some(usage) = &attempt.usage
                && let Some((elapsed, first)) = attempt.elapsed_us.zip(attempt.first_text_us)
                && elapsed > first
            {
                result.output_us = result.output_us.saturating_add(elapsed - first);
                result.output_tokens = result
                    .output_tokens
                    .saturating_add(usage.output.saturating_sub(usage.reasoning));
                result.output_samples += 1;
            }
            for row in &attempt.categories {
                let total = composition.entry(row.kind).or_insert_with(|| Category {
                    kind: row.kind,
                    items: Some(0),
                    tokens: Some(0),
                    method: row.method.clone(),
                });
                total.items = total
                    .items
                    .zip(row.items)
                    .map(|(sum, value)| sum.saturating_add(value));
                total.tokens = total
                    .tokens
                    .zip(row.tokens)
                    .map(|(sum, value)| sum.saturating_add(value));
            }
        }
    }
    result.composition = composition.into_values().collect();
    result.recent = calls
        .into_iter()
        .rev()
        .take(RECENT_LIMIT)
        .map(|(_, value)| value)
        .collect();
    result.recent.reverse();
    Ok(result)
}
