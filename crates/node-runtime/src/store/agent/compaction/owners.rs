//! Rehydrate captured owner fragments from canonical conversation membership.
use super::*;
use adk_core::Part;
use serde_json::Value;
use std::collections::HashMap;

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
    boundary: i64,
) -> Result<(HashMap<String, Value>, Vec<Part>), Fault> {
    let mut state = HashMap::new();
    {
        let mut statement = db
            .prepare(
                "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
                 WHERE h.session=?1 AND h.sequence<=?2
                   AND json_extract(e.body,'$.kind')!='compaction'
                   AND (json_type(e.body,'$.metadata.context')='object' OR json_extract(e.body,'$.kind')='configuration')
                 ORDER BY h.sequence",
            )
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query(params![session.to_string(), boundary])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
            let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
            let context = match &record.kind {
                Kind::Configuration(config) => Some(&config.context),
                _ => record.metadata.context.as_ref(),
            };
            if let Some(context) = context {
                state.extend(crate::agent::canonical_bridge::state(context));
            }
        }
    }
    let mut sources = Vec::new();
    {
        let mut statement = db
            .prepare(
                r#"SELECT e.body,json_extract(part.value,'$.index') FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence
             JOIN json_each(e.body,'$.content') part
             WHERE h.session=?1 AND h.sequence<=?2
             AND ((json_extract(part.value,'$.type')='tool_response' AND (json_extract(part.value,'$.name')
               IN ('load_skill','read_skill_resource','plan_review')
               OR json_type(part.value,'$.result.value.progress')='object'))
               OR (json_extract(e.body,'$.provenance.author')='user'
                   AND json_extract(part.value,'$.index')=json_extract(e.body,'$.metadata.input.capability_part')
                   AND h.sequence=(SELECT max(selected.sequence) FROM conversation_events selected
                     JOIN agent_events input ON input.sequence=selected.sequence
                     JOIN agent_runs selected_run ON selected_run.turn=input.turn
                     WHERE selected.session=?1 AND selected.sequence<=?2
                       AND json_extract(input.body,'$.provenance.author')='user'
                       AND json_type(input.body,'$.metadata.input.capability_part') IS NOT NULL
                       AND coalesce(json_extract(selected_run.body,'$.joined'),selected_run.turn)=(
                         SELECT coalesce(json_extract(latest_run.body,'$.joined'),latest_run.turn)
                         FROM conversation_events latest JOIN agent_events latest_input ON latest_input.sequence=latest.sequence
                         JOIN agent_runs latest_run ON latest_run.turn=latest_input.turn
                         WHERE latest.session=?1 AND json_extract(latest_input.body,'$.provenance.author')='user'
                           AND coalesce(json_extract(latest_run.body,'$.notice'),0)=0
                         ORDER BY latest.sequence DESC LIMIT 1))))
             ORDER BY h.sequence,json_extract(part.value,'$.index')"#,
            )
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query(params![session.to_string(), boundary])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
            let index = row.get::<i64>(1).map_err(storage_error)? as usize;
            let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
            let event = crate::agent::canonical_bridge::to_adk(db, &record).await?;
            if let Some(part) = event.content().and_then(|content| content.parts.get(index)) {
                sources.push(part.clone());
            }
        }
    }
    Ok(crate::agent::compose_context(state, &sources))
}
