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
            state.extend(
                serde_json::from_str::<HashMap<String, Value>>(&body).map_err(storage_error)?,
            );
        }
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
                   AND CAST(part.key AS TEXT)=json_extract(e.body,'$.event_metadata."sailry.context.capability_part"')))
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
