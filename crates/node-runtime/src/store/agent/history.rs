use super::*;
use serde_json::Value;
use std::collections::HashMap;

mod citations;
pub(super) mod pages;
mod web_search;

pub(super) fn append(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    mut event: AdkEvent,
    events: &broadcast::Sender<EventEnvelope>,
) -> adk_core::Result<()> {
    if event.llm_response.partial {
        return Err(AdkError::session("partial output is not durable history"));
    }
    if event.id.is_empty() || event.id.len() > 256 {
        return Err(AdkError::session("event identifier is invalid"));
    }
    event
        .actions
        .state_delta
        .retain(|key, _| !key.starts_with(adk_session::KEY_PREFIX_TEMP));
    // The canonical event keeps response metadata, not a second copy of model requests.
    event.llm_request = None;
    event
        .provider_metadata
        .remove("gcp.vertex.agent.llm_request");
    event
        .provider_metadata
        .remove("gcp.vertex.agent.llm_response");
    if event.llm_response.error_code.is_some() || event.llm_response.error_message.is_some() {
        event.llm_response.error_code = Some("provider_error".into());
        event.llm_response.error_message = Some("model response reported an error".into());
    }
    let body = encode(&event).map_err(adk_error)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(AdkError::session("event exceeds the history frame limit"));
    }
    let mut write = || -> Result<(), Fault> {
        let transaction = database.connection.transaction().map_err(storage_error)?;
        let owns: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE id=?1 AND session=?2)",
                params![turn.to_string(), session.to_string()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !owns {
            return Err(invalid("event turn belongs to another session"));
        }
        let previous: Option<(String, Vec<u8>)> = transaction
            .query_row(
                "SELECT e.turn,e.body FROM agent_events e JOIN conversation_turns h ON h.turn=e.turn WHERE h.session=?1 AND e.id=?2",
                params![session.to_string(), event.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((previous_turn, previous)) = previous {
            // JSON maps need not retain insertion order across a process restart.
            if previous_turn != turn.to_string()
                || serde_json::from_slice::<Value>(&previous).map_err(storage_error)?
                    != serde_json::from_slice::<Value>(&body).map_err(storage_error)?
            {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "event identifier already has different content",
                ));
            }
            return Ok(());
        }
        super::compaction::validate(&transaction, session, turn, &event)?;
        super::elicitation::validate(&transaction, turn, &event)?;
        transaction
            .execute(
                "INSERT INTO agent_events(session,id,turn,body) VALUES(?1,?2,?3,?4)",
                params![session.to_string(), event.id, turn.to_string(), body],
            )
            .map_err(storage_error)?;
        let sequence = transaction.last_insert_rowid() as u64;
        transaction
            .execute(
                "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)",
                params![session.to_string(), sequence as i64],
            )
            .map_err(storage_error)?;
        apply_state(&transaction, session, &event)?;
        let entry = project(&transaction, sequence, turn, &event)?;
        let statistics = super::statistics::read(&transaction, session)?;
        let envelope = changed(&transaction, database.node, session)?;
        transaction.commit().map_err(storage_error)?;
        let _ = events.send(envelope);
        database
            .feeds
            .publish(database.node, session, Change::Entry(entry));
        database
            .feeds
            .publish(database.node, session, Change::Statistics(statistics));
        Ok(())
    };
    write().map_err(adk_error)
}

fn apply_state(db: &Connection, session: SessionId, event: &AdkEvent) -> Result<(), Fault> {
    for (key, value) in &event.actions.state_delta {
        if key.starts_with(adk_session::KEY_PREFIX_TEMP) {
            continue;
        }
        let owner = if key.starts_with(adk_session::KEY_PREFIX_APP)
            || key.starts_with(adk_session::KEY_PREFIX_USER)
        {
            String::new()
        } else {
            session.to_string()
        };
        db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", params![owner, key, encode(value)?]).map_err(storage_error)?;
    }
    Ok(())
}

pub(super) fn state(db: &Connection, session: SessionId) -> Result<HashMap<String, Value>, Fault> {
    let mut query = db
        .prepare("SELECT key,body FROM agent_state WHERE owner='' OR owner=?1")
        .map_err(storage_error)?;
    query
        .query_map([session.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (key, body) = row.map_err(storage_error)?;
            Ok((key, serde_json::from_slice(&body).map_err(storage_error)?))
        })
        .collect()
}

pub(in crate::store) fn rebuild_session(db: &Connection, session: SessionId) -> Result<(), Fault> {
    // The current title is session metadata. Rewinding execution history must not
    // undo a rename, whether it came from the user or the agent naming tool.
    let named: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_state WHERE owner=?1 AND key=?2)",
            params![session.to_string(), SESSION_TITLE],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    db.execute(
        "DELETE FROM agent_state WHERE owner=?1 AND key!=?2",
        params![session.to_string(), SESSION_TITLE],
    )
    .map_err(storage_error)?;
    let mut query = db
        .prepare(
            "SELECT json_extract(e.body,'$.actions.state_delta') FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence",
        )
        .map_err(storage_error)?;
    for row in query
        .query_map([session.to_string()], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
    {
        let delta: HashMap<String, Value> =
            serde_json::from_str(&row.map_err(storage_error)?).map_err(storage_error)?;
        for (key, value) in delta {
            // Shared app/user state belongs to the Node, not a historical session branch.
            if (named && key == SESSION_TITLE)
                || key.starts_with(adk_session::KEY_PREFIX_TEMP)
                || key.starts_with(adk_session::KEY_PREFIX_APP)
                || key.starts_with(adk_session::KEY_PREFIX_USER)
            {
                continue;
            }
            db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body",
                params![session.to_string(), key, encode(&value)?]).map_err(storage_error)?;
        }
    }
    Ok(())
}

pub(in crate::store) fn rebuild_index(db: &Connection, session: SessionId) -> Result<(), Fault> {
    // Only references are indexed. The membership relation and canonical events rebuild this table.
    db.execute(
        "DELETE FROM conversation_events WHERE session=?1",
        [session.to_string()],
    )
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO conversation_events(session,sequence)
         SELECT h.session,e.sequence FROM conversation_turns h
         JOIN agent_events e ON e.turn=h.turn WHERE h.session=?1",
        [session.to_string()],
    )
    .map_err(storage_error)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn rebuild(db: &mut Connection) -> Result<(), Fault> {
    let transaction = db.transaction().map_err(storage_error)?;
    transaction
        .execute("DELETE FROM agent_state WHERE key!=?1", [SESSION_TITLE])
        .map_err(storage_error)?;
    {
        let mut query = transaction
            .prepare("SELECT session,body FROM agent_events ORDER BY sequence")
            .map_err(storage_error)?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (session, body) = row.map_err(storage_error)?;
            let mut event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
            event.actions.state_delta.remove(SESSION_TITLE);
            apply_state(
                &transaction,
                session.parse().map_err(storage_error)?,
                &event,
            )?;
        }
    }
    {
        let mut query = transaction
            .prepare("SELECT id FROM sessions WHERE state!='removed'")
            .map_err(storage_error)?;
        for row in query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
        {
            let session = row.map_err(storage_error)?.parse().map_err(storage_error)?;
            rebuild_index(&transaction, session)?;
            rebuild_session(&transaction, session)?;
        }
    }
    transaction.commit().map_err(storage_error)
}

pub(crate) fn project(
    db: &Connection,
    sequence: u64,
    turn: TurnId,
    event: &AdkEvent,
) -> Result<Entry, Fault> {
    let references = super::references::project(event)?;
    let presentations = super::presentation::read(event)?;
    let groupings = super::presentation::groupings(event)?;
    let displays = super::presentation::displays(event)?;
    Ok(Entry {
        sequence,
        turn,
        id: event.id.clone(),
        author: event.author.clone(),
        branch: event.branch.clone(),
        timestamp_ms: event.timestamp.timestamp_millis(),
        citations: citations::project(&event.llm_response),
        search_suggestions: event
            .llm_response
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata["searchEntryPoint"]["renderedContent"].as_str())
            .filter(|html| !html.is_empty())
            .map(str::to_owned),
        parts: event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
            .enumerate()
            .skip(usize::from(!references.is_empty()))
            .map(|(index, part)| {
                Ok(match part {
                    adk_core::Part::FileData {
                        file_uri,
                        mime_type,
                        ..
                    } if crate::agent::attachments::is_reference(file_uri) => {
                        let id = crate::agent::attachments::id(file_uri)?;
                        let attachment = super::super::attachments::turns::get(db, turn, id)?;
                        if attachment.spec.media_type != *mime_type {
                            return Err(invalid("attachment media type mismatch"));
                        }
                        Part::Attachment(attachment)
                    }
                    adk_core::Part::Text { text } => Part::Text(text.clone()),
                    adk_core::Part::InlineData { mime_type, data, .. }
                        if mime_type.starts_with("image/") =>
                    {
                        Part::Image(super::images::inline(db, turn, event, index, mime_type, data)?)
                    }
                    adk_core::Part::Thinking { thinking, .. } => Part::Thinking(thinking.clone()),
                    // Approval and question indices address canonical content parts.
                    // Hide provider commentary without shifting later tool calls.
                    adk_core::Part::ServerToolCall { server_tool_call }
                        if server_tool_call["type"] == "message"
                            && server_tool_call["phase"] == "commentary" => {
                        Part::Resource(serde_json::json!({"type": "provider_context"}))
                    }
                    adk_core::Part::ServerToolCall { server_tool_call } => {
                        web_search::call(server_tool_call, displays.get("web_search")).unwrap_or_else(|| {
                            Part::Resource(
                                serde_json::to_value(part).expect("ADK content is serializable"),
                            )
                        })
                    }
                    adk_core::Part::FunctionCall { id, name, args, .. } => Part::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: args.clone(),
                        presentation: presentations.get(name).copied().unwrap_or_default(),
                        grouping: groupings.get(name).copied().unwrap_or_default(),
                        display: displays.get(name).cloned().map(Box::new),
                    },
                    adk_core::Part::ServerToolResponse {
                        server_tool_response,
                    } => web_search::response(server_tool_response).unwrap_or_else(|| {
                        Part::Resource(
                            serde_json::to_value(part).expect("ADK content is serializable"),
                        )
                    }),
                    adk_core::Part::FunctionResponse {
                        id,
                        function_response,
                        ..
                    } => Part::ToolResult {
                        id: id.clone(),
                        name: function_response.name.clone(),
                        result: function_response.response.clone(),
                        images: super::images::project(db, turn, event, index, function_response)?,
                    },
                    part => Part::Resource(
                        serde_json::to_value(part).expect("ADK content is serializable"),
                    ),
                })
            })
            .chain(
                event
                    .content()
                    .into_iter()
                    .flat_map(|content| &content.parts)
                    .filter_map(|part| match part {
                        adk_core::Part::ServerToolCall { server_tool_call } => {
                            web_search::result(server_tool_call).map(Ok)
                        }
                        _ => None,
                    }),
            )
            .chain(event.actions.compaction.iter().map(|summary| {
                Ok(Part::Compaction(
                    summary
                        .compacted_content
                        .parts
                        .iter()
                        .filter_map(|part| part.text())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ))
            }))
            .chain(super::elicitation::project(event)?.into_iter().map(Ok))
            .chain(event.provider_metadata.get("adk_tool_started").map(|value| {
                let value: Value = serde_json::from_str(value).map_err(storage_error)?;
                let name = value["name"].as_str().ok_or_else(|| invalid("invalid tool start name"))?;
                Ok(Part::Resource(serde_json::json!({"type": "tool_started", "id": value["id"], "name": name})))
            }))
            .chain(event.provider_metadata.get("sailry_retry").map(|value| {
                let value: Value = serde_json::from_str(value).map_err(storage_error)?;
                let attempt = value["attempt"]
                    .as_u64()
                    .filter(|value| (1..=5).contains(value));
                let limit = value["limit"].as_u64().filter(|value| *value == 5);
                match (attempt, limit) {
                    (Some(attempt), Some(limit)) if event.author == "system" => Ok(Part::Resource(
                        serde_json::json!({"type":"model_retry","attempt":attempt,"limit":limit}),
                    )),
                    _ => Err(invalid("invalid model retry metadata")),
                }
            }))
            .chain(
                references
                    .into_iter()
                    .map(|reference| Ok(Part::Reference(reference))),
            )
            .collect::<Result<_, Fault>>()?,
        usage: event
            .llm_response
            .usage_metadata
            .as_ref()
            .map(|usage| Usage {
                input: usage.prompt_token_count.max(0) as u64,
                output: usage.candidates_token_count.max(0) as u64,
                cached_input: usage.cache_read_input_token_count.unwrap_or(0).max(0) as u64,
                reasoning: usage.thinking_token_count.unwrap_or(0).max(0) as u64,
            }),
    })
}

pub(super) const EVENTS_QUERY: &str =
    "SELECT e.sequence,e.turn,e.body,t.rowid FROM conversation_events h
    JOIN agent_events e ON e.sequence=h.sequence JOIN turns t ON t.id=e.turn
    WHERE h.session=?1 AND h.sequence<?2 ORDER BY h.sequence DESC LIMIT ?3";
