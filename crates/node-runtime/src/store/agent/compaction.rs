//! Derive model context from canonical membership, never wall-clock ordering.
use super::*;

pub(crate) const END_EVENT: &str = "sailry.context.end_event";

pub(super) fn validate(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    event: &AdkEvent,
) -> Result<(), Fault> {
    let Some(summary) = &event.actions.compaction else {
        return Ok(());
    };
    let end = event
        .provider_metadata
        .get(END_EVENT)
        .ok_or_else(|| invalid("context summary has no boundary"))?;
    if event.author != "system"
        || event.content().is_some()
        || summary.compacted_content.role != "model"
        || summary.compacted_content.parts.is_empty()
        || summary
            .compacted_content
            .parts
            .iter()
            .any(|part| !matches!(part, adk_core::Part::Text { text } if !text.trim().is_empty()))
    {
        return Err(invalid("context summary content is invalid"));
    }
    let valid: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND e.turn<>?3 AND json_extract(r.body,'$.status') NOT IN ('queued','running','stopping')
         AND NOT EXISTS(SELECT 1 FROM conversation_events later JOIN agent_events next ON next.sequence=later.sequence
           WHERE later.session=h.session AND next.turn=e.turn AND later.sequence>h.sequence
           AND json_type(next.body,'$.actions.compaction') IS NULL))",
        params![session.to_string(), end, turn.to_string()], |row| row.get(0)
    ).map_err(storage_error)?;
    if !valid && !current_exchange(db, session, turn, end)? {
        return Err(invalid(
            "context summary boundary is not a complete turn or tool exchange",
        ));
    }
    Ok(())
}

fn current_exchange(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    end: &str,
) -> Result<bool, Fault> {
    let sequence: Option<i64> = db.query_row(
        "SELECT e.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND e.turn=?3 AND t.kind='task'
         AND json_extract(r.body,'$.status')='running'",
        params![session.to_string(), end, turn.to_string()], |row| row.get(0)
    ).optional().map_err(storage_error)?;
    let Some(sequence) = sequence else {
        return Ok(false);
    };
    let mut statement = db
        .prepare(
            "SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
         AND json_type(body,'$.actions.compaction') IS NULL ORDER BY sequence",
        )
        .map_err(storage_error)?;
    let events = statement
        .query_map(params![turn.to_string(), sequence], |row| {
            row.get::<_, Vec<u8>>(0)
        })
        .map_err(storage_error)?
        .map(|body| {
            serde_json::from_slice::<AdkEvent>(&body.map_err(storage_error)?).map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(complete_exchange(&events))
}

pub(crate) fn complete_exchange(events: &[AdkEvent]) -> bool {
    use adk_core::Part;
    use std::collections::HashMap;
    let mut pending = HashMap::new();
    for part in events
        .iter()
        .filter_map(AdkEvent::content)
        .flat_map(|content| &content.parts)
    {
        match part {
            Part::FunctionCall { id, name, .. } => {
                *pending.entry((id, name)).or_insert(0usize) += 1;
            }
            Part::FunctionResponse {
                id,
                function_response,
                ..
            } => {
                let key = (id, &function_response.name);
                let Some(count) = pending.get_mut(&key) else {
                    return false;
                };
                *count -= 1;
                if *count == 0 {
                    pending.remove(&key);
                }
            }
            _ => {}
        }
    }
    pending.is_empty()
        && events
            .last()
            .and_then(AdkEvent::content)
            .is_some_and(|content| {
                content
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::FunctionResponse { .. }))
            })
}

pub(super) fn read(
    db: &Connection,
    session: SessionId,
) -> adk_core::Result<Option<(i64, AdkEvent)>> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND json_type(e.body,'$.actions.compaction')='object'
         ORDER BY h.sequence DESC LIMIT 1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| adk_error(storage_error(error)))?;
    let Some(body) = body else { return Ok(None) };
    let mut event: AdkEvent =
        serde_json::from_slice(&body).map_err(|error| adk_error(storage_error(error)))?;
    let end = event
        .provider_metadata
        .get(END_EVENT)
        .ok_or_else(|| AdkError::session("context summary has no boundary"))?;
    let sequence: i64 = db.query_row(
        "SELECT h.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2",
        params![session.to_string(), end], |row| row.get(0)
    ).map_err(|error| adk_error(storage_error(error)))?;
    let summary = event
        .actions
        .compaction
        .take()
        .expect("selected compaction event");
    // ADK normally filters by timestamps. The Node instead resolves the exact
    // canonical event, so clock regressions and inherited turns cannot hide work.
    event.author = "assistant".into();
    event.set_content(summary.compacted_content);
    event.llm_response.usage_metadata = None;
    Ok(Some((sequence, event)))
}
