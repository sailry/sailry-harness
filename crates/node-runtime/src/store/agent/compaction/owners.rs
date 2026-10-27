//! Rehydrate captured owner fragments from canonical conversation membership.
use super::*;
use adk_core::Part;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
    boundary: i64,
) -> Result<(HashMap<String, Value>, Vec<Part>), Fault> {
    let mut state = HashMap::new();
    let mut loaded = BTreeSet::new();
    {
        let mut statement = db
            .prepare(
                "SELECT json_extract(e.body,'$.actions.state_delta')
             FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
             WHERE h.session=?1 AND h.sequence<=?2
             AND json_type(e.body,'$.actions.compaction') IS NULL
             AND EXISTS(SELECT 1 FROM json_each(e.body,'$.actions.state_delta') field
               WHERE field.key IN ('sailry.context.declarations','sailry.context.policy',
                 'sailry.context.environment','sailry.context.personal','sailry.context.loaded_tools','sailry.context.masked_outputs')
               OR field.key GLOB 'sailry.context.guide:*')
             ORDER BY h.sequence",
            )
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query(params![session.to_string(), boundary])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body = row.get::<String>(0).map_err(storage_error)?;
            let mut delta: HashMap<String, Value> =
                serde_json::from_str(&body).map_err(storage_error)?;
            if let Some(value) = delta.remove(LOADED_TOOLS) {
                loaded.extend(
                    serde_json::from_value::<BTreeSet<String>>(value).map_err(storage_error)?,
                );
            }
            state.extend(delta);
        }
    }
    loaded.extend(history::called(db, session, boundary).await?);
    if !loaded.is_empty() {
        state.insert(LOADED_TOOLS.into(), serde_json::json!(loaded));
    }
    let mut sources = Vec::new();
    {
        let mut statement = db
            .prepare(
                r#"SELECT part.value FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence
             JOIN json_each(e.body,'$.content.parts') part
             WHERE h.session=?1 AND h.sequence<=?2
             AND (json_extract(part.value,'$.functionResponse.name')
               IN ('load_skill','read_skill_resource','plan_review')
               OR json_type(part.value,'$.functionResponse.response.progress')='object'
               OR (json_extract(e.body,'$.author')='user'
                   AND CAST(part.key AS TEXT)=json_extract(e.body,'$.event_metadata."sailry.context.capability_part"')
                   AND h.sequence=(SELECT max(latest.sequence) FROM conversation_events latest
                     JOIN agent_events input ON input.sequence=latest.sequence
                     WHERE latest.session=?1 AND json_extract(input.body,'$.author')='user')))
             ORDER BY h.sequence,CAST(part.key AS INTEGER)"#,
            )
            .await
            .map_err(storage_error)?;
        let mut rows = statement
            .query(params![session.to_string(), boundary])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body = row.get::<String>(0).map_err(storage_error)?;
            sources.push(serde_json::from_str(&body).map_err(storage_error)?);
        }
    }
    Ok(crate::agent::compose_context(state, &sources))
}
