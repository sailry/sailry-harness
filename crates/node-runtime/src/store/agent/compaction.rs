//! Derive model context from canonical membership, never wall-clock ordering.
use super::*;
mod owners;

pub(crate) const END_EVENT: &str = "sailry.context.end_event";

pub(super) async fn validate(
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
        .ok_or_else(|| sailry_protocol::Fault::invalid("context summary has no boundary"))?;
    if event.author != "system"
        || event.content().is_some()
        || summary.compacted_content.role != "user"
        || summary.compacted_content.parts.is_empty()
        || summary
            .compacted_content
            .parts
            .iter()
            .any(|part| !matches!(part, adk_core::Part::Text { text } if !text.trim().is_empty()))
    {
        return Err(sailry_protocol::Fault::invalid(
            "context summary content is invalid",
        ));
    }
    let visible = history::VISIBLE_EVENT;
    let later_visible = visible.replace("e.body", "next.body");
    let valid: bool = {
        let mut statement = db.prepare(&format!("SELECT EXISTS(SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND e.turn<>?3 AND json_extract(r.body,'$.status') NOT IN ('queued','running','stopping')
         AND {visible}
         AND NOT EXISTS(SELECT 1 FROM conversation_events later JOIN agent_events next ON next.sequence=later.sequence
           WHERE later.session=h.session AND next.turn=e.turn AND later.sequence>h.sequence
           AND json_type(next.body,'$.actions.compaction') IS NULL AND {later_visible}))")).await.map_err(storage_error)?;
        let row = statement
            .query_row(params![session.to_string(), end.as_str(), turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if !valid && !current_exchange(db, session, turn, end).await? {
        return Err(sailry_protocol::Fault::invalid(
            "context summary boundary is not a complete turn or tool exchange",
        ));
    }
    Ok(())
}

async fn current_exchange(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    end: &str,
) -> Result<bool, Fault> {
    let visible = history::VISIBLE_EVENT;
    let boundary: Option<(i64, String)> = {
        let mut statement = db.prepare(&format!("SELECT e.sequence,e.turn FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN turns t ON t.id=e.turn JOIN agent_runs r ON r.turn=t.id
         WHERE h.session=?1 AND e.id=?2 AND t.kind='task' AND {visible}
         AND ((e.turn=?3 AND json_extract(r.body,'$.status')='running')
           OR json_extract(r.body,'$.status') NOT IN ('queued','running','stopping'))")).await.map_err(storage_error)?;
        match statement
            .query_row(params![session.to_string(), end, turn.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some((sequence, owner)) = boundary else {
        return Ok(false);
    };
    let mut statement = db
        .prepare(
            "SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
         AND json_type(body,'$.actions.compaction') IS NULL ORDER BY sequence",
        )
        .await
        .map_err(storage_error)?;
    let events = {
        let mut rows = statement
            .query(params![owner, sequence])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|body| {
        serde_json::from_slice::<AdkEvent>(&body.map_err(storage_error)?).map_err(storage_error)
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(complete_exchange(&events))
}

#[derive(Default)]
pub(crate) struct Exchange {
    pending: std::collections::HashMap<(Option<String>, String), usize>,
    invalid: bool,
    complete: bool,
}

impl Exchange {
    pub(crate) fn push(&mut self, event: &AdkEvent) -> bool {
        if event.content().is_none() {
            return self.complete;
        }
        let mut response = false;
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            match part {
                adk_core::Part::FunctionCall { id, name, .. } => {
                    *self.pending.entry((id.clone(), name.clone())).or_default() += 1;
                }
                adk_core::Part::FunctionResponse {
                    id,
                    function_response,
                    ..
                } => {
                    response = true;
                    let key = (id.clone(), function_response.name.clone());
                    if let Some(count) = self.pending.get_mut(&key) {
                        *count -= 1;
                        if *count == 0 {
                            self.pending.remove(&key);
                        }
                    } else {
                        self.invalid = true;
                    }
                }
                _ => {}
            }
        }
        self.complete = !self.invalid && response && self.pending.is_empty();
        self.complete
    }
}

pub(crate) fn complete_exchange(events: &[AdkEvent]) -> bool {
    let mut exchange = Exchange::default();
    let mut complete = false;
    for event in events {
        complete = exchange.push(event);
    }
    complete
}

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
) -> adk_core::Result<Option<(i64, AdkEvent)>> {
    let body: Option<Vec<u8>> = (match (async { let mut statement = db.prepare("SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND json_type(e.body,'$.actions.compaction')='object'
         ORDER BY h.sequence DESC LIMIT 1").await?;
let row = statement.query_row([session.to_string()]).await?;
let mapped: turso::Result<_> = row.get(0);
mapped }).await { Ok(value) => Ok(Some(value)), Err(turso::Error::QueryReturnedNoRows) => Ok(None), Err(error) => Err(error) })
        .map_err(|error| adk_error(storage_error(error)))?;
    let Some(body) = body else { return Ok(None) };
    let mut event: AdkEvent =
        serde_json::from_slice(&body).map_err(|error| adk_error(storage_error(error)))?;
    let end = event
        .provider_metadata
        .get(END_EVENT)
        .ok_or_else(|| AdkError::session("context summary has no boundary"))?;
    let sequence: i64 = (async { let mut statement = db.prepare("SELECT h.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2").await?;
let row = statement.query_row(params![session.to_string(), end.as_str()]).await?;
let mapped: turso::Result<_> = row.get(0);
mapped }).await.map_err(|error| adk_error(storage_error(error)))?;
    let summary = event
        .actions
        .compaction
        .take()
        .expect("selected compaction event");
    // ADK normally filters by timestamps. The Node instead resolves the exact
    // canonical event, so clock regressions and inherited turns cannot hide work.
    event.author = "user".into();
    let mut content = summary.compacted_content;
    content.role = "user".into();
    let (state, mut parts) = owners::read(db, session, sequence)
        .await
        .map_err(adk_error)?;
    content.parts.append(&mut parts);
    event.actions.state_delta.extend(state);
    event.set_content(content);
    event.llm_response.usage_metadata = None;
    Ok(Some((sequence, event)))
}

/// A derived model window with boundaries from canonical turn ownership.
/// Synthetic invocation IDs and wall clocks never establish a turn boundary.
pub(crate) struct History {
    pub events: Vec<AdkEvent>,
    pub completed: Vec<usize>,
    pub current: usize,
    pub recent: Option<usize>,
}

pub(super) async fn history(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> adk_core::Result<History> {
    let events = sessions::get(
        db,
        turn,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .await?
    .events()
    .all();
    // Prompt events omit telemetry; their completed boundary must use the same
    // projection rather than a later statistics-only canonical event.
    let visible = history::VISIBLE_EVENT;
    let mut query = db
        .prepare(&format!(
            "WITH ends AS (SELECT e.turn, max(e.sequence) AS last FROM conversation_events h
           JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1
           AND json_type(e.body,'$.actions.compaction') IS NULL
           AND json_type(e.body,'$.content')='object' AND {visible} GROUP BY e.turn)
         SELECT e.id, e.turn, json_extract(r.body,'$.status'), e.sequence=ends.last
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN agent_runs r ON r.turn=e.turn JOIN ends ON ends.turn=e.turn WHERE h.session=?1"
        ))
        .await
        .map_err(|error| adk_error(storage_error(error)))?;
    let owners = {
        let mut rows = query
            .query([session.to_string()])
            .await
            .map_err(|error| adk_error(storage_error(error)))?;
        let mut values = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| adk_error(storage_error(error)))?
        {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<String>(0)?,
                    (
                        row.get::<String>(1)?,
                        row.get::<String>(2)?,
                        row.get::<bool>(3)?,
                    ),
                ))
            })(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<std::collections::HashMap<_, _>, _>>()
    .map_err(|error| adk_error(storage_error(error)))?;
    let current = events
        .iter()
        .position(|event| {
            !event.provider_metadata.contains_key(END_EVENT)
                && owners
                    .get(&event.id)
                    .is_some_and(|(owner, _, _)| *owner == turn.to_string())
        })
        .unwrap_or(events.len());
    let mut completed = Vec::new();
    let mut starts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut exchanges = std::collections::HashMap::<String, Exchange>::new();
    for (index, event) in events.iter().enumerate() {
        // Failure notices and other metadata do not create a recent interaction
        // or hide the last model-visible boundary of an existing one.
        if event.content().is_none() {
            continue;
        }
        let Some((owner, status, last)) = owners.get(&event.id) else {
            continue;
        };
        if !event.provider_metadata.contains_key(END_EVENT)
            && !matches!(status.as_str(), "queued" | "running" | "stopping")
            && seen.insert(owner.clone())
        {
            starts.push(index);
        }
        let exchange = exchanges.entry(owner.clone()).or_default().push(event);
        if !matches!(status.as_str(), "queued" | "running" | "stopping") && (*last || exchange) {
            completed.push(index + 1);
        }
    }
    Ok(History {
        events,
        completed,
        current,
        recent: starts.iter().rev().nth(1).copied(),
    })
}
