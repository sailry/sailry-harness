use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(in crate::store::agent) mod canonical;
mod citations;
mod credentials;
mod interrupted;
mod settled;
pub(super) use interrupted::retain;
pub(in crate::store) mod output;
pub(super) mod pages;
pub(in crate::store) mod tools;
// Canonical telemetry remains available to statistics without consuming chat or prompt pages.
pub(super) const VISIBLE_EVENT: &str = "CASE WHEN
    json_extract(e.body,'$.author')='system'
    AND json_extract(e.body,'$.content') IS NULL
THEN NOT coalesce((
    (json_extract(e.body,'$.usage_metadata') IS NULL
         OR (json_extract(e.body,'$.event_metadata.sailry_model_purpose')='compaction'
             AND json_type(e.body,'$.event_metadata.sailry_model_observation') IS 'text'))
    AND json_extract(e.body,'$.error_code') IS NULL
    AND json_extract(e.body,'$.error_message') IS NULL
    AND json_extract(e.body,'$.turn_complete')=0
    AND json_extract(e.body,'$.interrupted')=0
    AND json_extract(e.body,'$.long_running_tool_ids')='[]'
    AND (json_type(e.body,'$.event_metadata.sailry_model_observation') IS 'text'
         OR json_type(e.body,'$.event_metadata.sailry_execution_phases') IS 'text'
         OR json_type(e.body,'$.event_metadata.sailry_context_projection') IS 'text')
    AND NOT EXISTS (SELECT 1 FROM json_each(e.body,'$.event_metadata') metadata
        WHERE metadata.key NOT IN ('sailry_model_observation','sailry_execution_phases','sailry_model_purpose','sailry_context_projection'))
    AND NOT EXISTS (SELECT 1 FROM json_each(e.body,'$.actions.state_delta') field
        WHERE field.key!='sailry.context.masked_outputs')
    AND json_extract(e.body,'$.actions.artifact_delta')='{}'
    AND NOT EXISTS (SELECT 1 FROM json_each(e.body,'$.actions') action WHERE NOT (
        action.type='null'
        OR action.key='state_delta'
        OR (action.key IN ('artifact_delta','workflow_positions') AND action.value='{}')
        OR (action.key IN ('skip_summarization','escalate','input_screened') AND action.value=0)
    ))
),0) ELSE 1 END";
#[cfg(test)]
mod tests;
mod web_search;

pub(super) fn commentary(value: &Value) -> Option<String> {
    if value["type"] != "message" || value["phase"] != "commentary" {
        return None;
    }
    Some(
        value["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|part| part["type"] == "output_text")
            .filter_map(|part| part["text"].as_str())
            .collect(),
    )
}

pub(super) async fn append(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    event: AdkEvent,
    events: &broadcast::Sender<EventEnvelope>,
) -> adk_core::Result<()> {
    append_with_context(database, session, turn, event, None, events).await
}

pub(super) async fn append_with_context(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    event: AdkEvent,
    context: Option<(TurnId, AdkEvent)>,
    events: &broadcast::Sender<EventEnvelope>,
) -> adk_core::Result<()> {
    let permit = database.write().await.map_err(adk_error)?;
    for attempt in 0..=crate::store::database::conflict::RETRIES {
        let event = event.clone();
        let context = context.clone();
        let write = async || -> Result<(), Fault> {
            let publication = database.publication.clone();
            let transaction = crate::store::database::conflict::begin(&database.connection).await?;
            let prepared: Result<_, Fault> = async {
                if event
                    .provider_metadata
                    .contains_key(crate::agent::CONTEXT_METADATA)
                    && runs::get(&transaction, turn).await?.status != Status::Running
                {
                    return Err(Fault::new(
                        ErrorCode::Conflict,
                        "context recipient is no longer running",
                    ));
                }
                let context = if let Some((owner, event)) = context {
                    if runs::get(&transaction, owner).await?.status != Status::Running {
                        return Err(Fault::new(
                            ErrorCode::Conflict,
                            "guidance recipient is no longer running",
                        ));
                    }
                    insert(&transaction, session, owner, event).await?
                } else {
                    None
                };
                let Some(entry) = insert(&transaction, session, turn, event).await? else {
                    return Ok(None);
                };
                let visible: bool = {
                    let mut statement = transaction
                        .prepare(&format!(
                            "SELECT {VISIBLE_EVENT} FROM agent_events e WHERE e.sequence=?1"
                        ))
                        .await
                        .map_err(storage_error)?;
                    let row = statement
                        .query_row([i64::try_from(entry.sequence).map_err(storage_error)?])
                        .await
                        .map_err(storage_error)?;
                    let mapped: turso::Result<_> = row.get(0);
                    mapped.map_err(storage_error)?
                };
                let notice =
                    super::super::notifications::system::compaction(&transaction, session, &entry)
                        .await?;
                let background = super::background::list(&transaction, session).await?;
                let statistics = super::statistics::read(&transaction, session).await?;
                let parents = {
                    let mut selected = Vec::new();
                    for value in super::statistics::parents(&transaction, turn)
                        .await?
                        .into_iter()
                    {
                        selected.push(
                            async {
                                let parent = value;
                                super::statistics::read(&transaction, parent)
                                    .await
                                    .map(|statistics| (parent, statistics))
                            }
                            .await?,
                        );
                    }
                    Ok::<_, Fault>(selected)
                }?;
                let envelope = changed(&transaction, database.node, session).await?;
                let mut changes: Vec<_> = notice.into_iter().collect();
                if settled::completed(&transaction, turn, &entry).await? {
                    changes.push(Event::ToolsCompleted { session });
                }
                let notices =
                    super::super::database::record(&transaction, database.node, changes).await?;
                Ok(Some((
                    entry, visible, context, background, statistics, parents, envelope, notices,
                )))
            }
            .await;
            let Some((entry, visible, context, background, statistics, parents, envelope, notices)) =
                prepared?
            else {
                return Ok(());
            };
            let publication = publication.lock().await;
            crate::store::database::conflict::commit(transaction).await?;
            publication.send(events, envelope);
            for notice in notices {
                publication.send(events, notice);
            }
            if let Some(context) = context {
                database
                    .feeds
                    .publish(database.node, session, Change::Entry(context));
            }
            if visible {
                database
                    .feeds
                    .publish(database.node, session, Change::Entry(entry));
            }
            database.feeds.publish(
                database.node,
                session,
                Change::Statistics(Box::new(statistics)),
            );
            super::background::publish(database, background).await;
            for (parent, statistics) in parents {
                database.feeds.publish(
                    database.node,
                    parent,
                    Change::Statistics(Box::new(statistics)),
                );
            }
            Ok(())
        };
        let result = write().await;
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(permit);
            return Err(adk_error(error));
        }
        if !crate::store::database::conflict::again(&result, attempt).await {
            return result.map_err(adk_error);
        }
    }
    unreachable!("the final attempt returns its result")
}

/// Insert into the caller's existing transaction; this is the same history owner.
pub(super) async fn insert(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    mut event: AdkEvent,
) -> Result<Option<Entry>, Fault> {
    if event.llm_response.partial {
        return Err(Fault::invalid("partial output is not durable history"));
    }
    if event.id.is_empty() || event.id.len() > 256 {
        return Err(Fault::invalid("event identifier is invalid"));
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
    super::approvals::results::append(db, turn, &mut event).await?;
    let previous: Option<(String, Vec<u8>)> = {
        let mut statement = db.prepare("SELECT e.turn,e.body FROM agent_events e JOIN conversation_turns h ON h.turn=e.turn WHERE h.session=?1 AND e.id=?2").await.map_err(storage_error)?;
        match statement
            .query_row(params![session.to_string(), event.id.as_str()])
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
    if previous.is_none() && event.author == "user" {
        let input = super::runs::get(db, turn).await?;
        if let Some(owner) = input.joined
            && super::runs::get(db, owner).await?.status != Status::Running
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "guidance recipient is no longer running",
            ));
        }
    }
    #[cfg(test)]
    tests::before_write().await?;
    super::presentation::preserve_failures(
        &mut event,
        previous.as_ref().map(|(_, body)| body.as_slice()),
    )?;
    super::presentation::capture_names(
        db,
        session,
        &mut event,
        previous.as_ref().map(|(_, body)| body.as_slice()),
    )
    .await?;
    if let Some(value) = event.provider_metadata.get("sailry_model_observation") {
        let mut request: sailry_protocol::conversation::observation::Request =
            serde_json::from_str(value).map_err(storage_error)?;
        let first: Option<String> = {
            let mut statement = db.prepare("SELECT json_extract(e.body,'$.event_metadata.sailry_model_observation') FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1
             AND json_extract(json_extract(e.body,'$.event_metadata.sailry_model_observation'),'$.id')=?2 ORDER BY h.sequence LIMIT 1").await.map_err(storage_error)?;
            match statement
                .query_row(params![session.to_string(), request.id.as_str()])
                .await
            {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        request.boundary = if let Some(first) = first {
            serde_json::from_str::<sailry_protocol::conversation::observation::Request>(&first)
                .map_err(storage_error)?
                .boundary
        } else {
            {
                let mut statement = db
                    .prepare("SELECT max(sequence) FROM conversation_events WHERE session=?1")
                    .await
                    .map_err(storage_error)?;
                let row = statement
                    .query_row([session.to_string()])
                    .await
                    .map_err(storage_error)?;
                let mapped: turso::Result<_> = row.get::<Option<i64>>(0);
                mapped.map_err(storage_error)?
            }
            .map(u64::try_from)
            .transpose()
            .map_err(storage_error)?
        };
        event.provider_metadata.insert(
            "sailry_model_observation".into(),
            serde_json::to_string(&request).map_err(storage_error)?,
        );
    }
    let body = encode(&event)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(Fault::invalid("event exceeds the history frame limit"));
    }
    let owns: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM turns WHERE id=?1 AND session=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if !owns {
        return Err(Fault::invalid("event turn belongs to another session"));
    }
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
        return Ok(None);
    }
    super::compaction::validate(db, session, turn, &event).await?;
    super::elicitation::validate(db, turn, &event).await?;
    super::review::validate(db, turn, &event).await?;
    let sequence = super::super::database::counters::next(
        db,
        super::super::database::counters::Namespace::AgentEvents,
    )
    .await?;
    db.execute(
        "INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(?1,?2,?3,?4,?5)",
        params![
            sequence as i64,
            session.to_string(),
            event.id.as_str(),
            turn.to_string(),
            body
        ],
    )
    .await
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)",
        params![session.to_string(), sequence as i64],
    )
    .await
    .map_err(storage_error)?;
    super::background::confirm(db, session, turn, &event).await?;
    apply_state(db, session, &event).await?;
    if event.author == "user" {
        // Pending input becomes delivered only with its authoritative user entry.
        db.execute(
            "DELETE FROM agent_pending WHERE turn=?1",
            [turn.to_string()],
        )
        .await
        .map_err(storage_error)?;
    }
    Ok(Some(project(db, sequence, turn, &event).await?))
}

async fn apply_state(db: &Connection, session: SessionId, event: &AdkEvent) -> Result<(), Fault> {
    for (key, value) in &event.actions.state_delta {
        if key.starts_with(adk_session::KEY_PREFIX_TEMP) || key == LOADED_TOOLS {
            continue;
        }
        let owner = if key.starts_with(adk_session::KEY_PREFIX_APP)
            || key.starts_with(adk_session::KEY_PREFIX_USER)
        {
            String::new()
        } else {
            session.to_string()
        };
        db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", params![owner, key.as_str(), encode(value)?]).await.map_err(storage_error)?;
    }
    let mut names = event
        .actions
        .state_delta
        .get(LOADED_TOOLS)
        .map(|value| {
            serde_json::from_value::<BTreeSet<String>>(value.clone()).map_err(storage_error)
        })
        .transpose()?
        .unwrap_or_default();
    names.extend(crate::agent::called_tools(event.content()));
    remember(db, session, names).await
}

pub(in crate::store) async fn remember(
    db: &Connection,
    session: SessionId,
    names: BTreeSet<String>,
) -> Result<(), Fault> {
    if names.is_empty() {
        return Ok(());
    }
    let mut query = db
        .prepare("SELECT body FROM agent_state WHERE owner=?1 AND key=?2")
        .await
        .map_err(storage_error)?;
    let mut loaded: BTreeSet<String> = match query
        .query_row(params![session.to_string(), LOADED_TOOLS])
        .await
    {
        Ok(row) => serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error)?,
        Err(turso::Error::QueryReturnedNoRows) => BTreeSet::new(),
        Err(error) => return Err(storage_error(error)),
    };
    drop(query);
    let previous = loaded.len();
    loaded.extend(names);
    if loaded.len() != previous {
        db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body",
            params![session.to_string(), LOADED_TOOLS, encode(&loaded)?]).await.map_err(storage_error)?;
    }
    Ok(())
}

pub(in crate::store) async fn missing_tools(
    db: &Connection,
) -> Result<BTreeMap<SessionId, BTreeSet<String>>, Fault> {
    let mut loaded = BTreeMap::<String, BTreeSet<String>>::new();
    let mut query = db
        .prepare("SELECT owner,body FROM agent_state WHERE key=?1")
        .await
        .map_err(storage_error)?;
    let mut rows = query.query([LOADED_TOOLS]).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        loaded.insert(
            row.get(0).map_err(storage_error)?,
            serde_json::from_slice(&row.get::<Vec<u8>>(1).map_err(storage_error)?)
                .map_err(storage_error)?,
        );
    }
    drop(rows);
    drop(query);
    let mut query = db
        .prepare(
            "SELECT DISTINCT h.session,json_extract(part.value,'$.name')
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN json_each(e.body,'$.content.parts') part
         WHERE json_type(part.value,'$.name')='text' AND json_type(part.value,'$.args') IS NOT NULL",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query.query(()).await.map_err(storage_error)?;
    let mut missing = BTreeMap::<SessionId, BTreeSet<String>>::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let session = row.get::<String>(0).map_err(storage_error)?;
        let name = row.get::<String>(1).map_err(storage_error)?;
        if loaded
            .get(&session)
            .is_some_and(|names| names.contains(&name))
        {
            continue;
        }
        missing
            .entry(session.parse().map_err(storage_error)?)
            .or_default()
            .insert(name);
    }
    Ok(missing)
}

pub(in crate::store::agent) async fn called(
    db: &Connection,
    session: SessionId,
    boundary: i64,
) -> Result<BTreeSet<String>, Fault> {
    let mut query = db
        .prepare(
            "SELECT DISTINCT json_extract(part.value,'$.name')
        FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
        JOIN json_each(e.body,'$.content.parts') part WHERE h.session=?1 AND h.sequence<=?2
        AND json_type(part.value,'$.name')='text' AND json_type(part.value,'$.args') IS NOT NULL",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), boundary])
        .await
        .map_err(storage_error)?;
    let mut names = BTreeSet::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        names.insert(row.get::<String>(0).map_err(storage_error)?);
    }
    Ok(names)
}

pub(super) async fn state(
    db: &Connection,
    session: SessionId,
) -> Result<HashMap<String, Value>, Fault> {
    let mut query = db
        .prepare("SELECT key,body FROM agent_state WHERE owner='' OR owner=?1")
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query
            .query([session.to_string()])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| {
        let (key, body) = row.map_err(storage_error)?;
        Ok((key, serde_json::from_slice(&body).map_err(storage_error)?))
    })
    .collect()
}

pub(in crate::store) async fn rebuild_session(
    db: &Connection,
    session: SessionId,
) -> Result<(), Fault> {
    // The current title is session metadata. Rewinding execution history must not
    // undo a rename, whether it came from the user or the agent naming tool.
    let named: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM agent_state WHERE owner=?1 AND key=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![session.to_string(), SESSION_TITLE])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    db.execute(
        "DELETE FROM agent_state WHERE owner=?1 AND key!=?2",
        params![session.to_string(), SESSION_TITLE],
    )
    .await
    .map_err(storage_error)?;
    let mut query = db
        .prepare(
            "SELECT json_extract(e.body,'$.actions.state_delta') FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence",
        )
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query
            .query([session.to_string()])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let row: turso::Result<_> = row.get::<String>(0);

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
                if key == LOADED_TOOLS {
                    remember(
                        db,
                        session,
                        serde_json::from_value(value).map_err(storage_error)?,
                    )
                    .await?;
                    continue;
                }
                db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body",
                params![session.to_string(), key.as_str(), encode(&value)?]).await.map_err(storage_error)?;
            }
        }
    }
    drop(query);
    remember(db, session, called(db, session, i64::MAX).await?).await?;
    Ok(())
}

pub(in crate::store) async fn rebuild_index(
    db: &Connection,
    session: SessionId,
) -> Result<(), Fault> {
    // Only references are indexed. The membership relation and canonical events rebuild this table.
    db.execute(
        "DELETE FROM conversation_events WHERE session=?1",
        [session.to_string()],
    )
    .await
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO conversation_events(session,sequence)
         SELECT h.session,e.sequence FROM conversation_turns h
         JOIN agent_events e ON e.turn=h.turn WHERE h.session=?1",
        [session.to_string()],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

#[cfg(test)]
pub(super) async fn rebuild(db: &mut Connection) -> Result<(), Fault> {
    let transaction = db.transaction().await.map_err(storage_error)?;
    transaction
        .execute("DELETE FROM agent_state WHERE key!=?1", [SESSION_TITLE])
        .await
        .map_err(storage_error)?;
    {
        let mut query = transaction
            .prepare("SELECT session,body FROM agent_events ORDER BY sequence")
            .await
            .map_err(storage_error)?;
        let rows = {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
                values.push(mapped);
            }
            values.into_iter()
        };
        for row in rows {
            let (session, body) = row.map_err(storage_error)?;
            let mut event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
            event.actions.state_delta.remove(SESSION_TITLE);
            apply_state(
                &transaction,
                session.parse().map_err(storage_error)?,
                &event,
            )
            .await?;
        }
    }
    {
        let mut query = transaction
            .prepare("SELECT id FROM sessions WHERE state!='removed'")
            .await
            .map_err(storage_error)?;
        {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let row: turso::Result<_> = row.get::<String>(0);

                let session = row.map_err(storage_error)?.parse().map_err(storage_error)?;
                rebuild_index(&transaction, session).await?;
                rebuild_session(&transaction, session).await?;
            }
        }
    }
    transaction.commit().await.map_err(storage_error)
}

pub(crate) async fn project(
    db: &Connection,
    sequence: u64,
    turn: TurnId,
    event: &AdkEvent,
) -> Result<Entry, Fault> {
    project_with_sources(db, sequence, turn, event, None).await
}

async fn project_with_sources(
    db: &Connection,
    sequence: u64,
    turn: TurnId,
    event: &AdkEvent,
    sources: Option<&tools::Sources>,
) -> Result<Entry, Fault> {
    let references = super::references::project(event)?;
    let displays = super::presentation::displays(event)?;
    let failures = super::presentation::failures(event)?;
    let skipped = usize::from(!references.is_empty())
        + usize::from(
            event
                .provider_metadata
                .contains_key(super::background::INPUT_IDS),
        );
    let hidden = event.author == "system"
        && event
            .provider_metadata
            .contains_key(crate::agent::CONTEXT_METADATA);
    let capability_part = event
        .provider_metadata
        .get(super::CAPABILITY_PART)
        .and_then(|value| value.parse::<usize>().ok());
    let commentary = if hidden {
        Default::default()
    } else {
        event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
            .enumerate()
            .skip(skipped)
            .filter(|(index, _)| Some(*index) != capability_part)
            .filter_map(|(index, part)| {
                let adk_core::Part::ServerToolCall { server_tool_call } = part else {
                    return None;
                };
                super::history::commentary(server_tool_call)?;
                Some((
                    index - skipped,
                    server_tool_call["id"]
                        .as_str()
                        .unwrap_or(&event.id)
                        .to_owned(),
                ))
            })
            .collect()
    };
    Ok(Entry {
        sequence,
        turn,
        id: event.id.clone(),
        author: event.author.clone(),
        branch: event.branch.clone(),
        timestamp_ms: event.timestamp.timestamp_millis(),
        commentary,
        citations: citations::project(&event.llm_response),
        search_suggestions: event
            .llm_response
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata["searchEntryPoint"]["renderedContent"].as_str())
            .filter(|html| !html.is_empty())
            .map(str::to_owned),
        parts: if hidden {
            Vec::new()
        } else {
            { let mut projected = Vec::new(); for value in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
            .enumerate()
            .skip(skipped)
            .filter(|(index, _)| Some(*index) != capability_part) { projected.push((async |(index, part): (usize, &adk_core::Part)| {
                Ok(match part {
                    adk_core::Part::FileData {
                        file_uri,
                        mime_type,
                        ..
                    } if crate::agent::attachments::is_reference(file_uri) => {
                        let id = crate::agent::attachments::id(file_uri)?;
                        let attachment = super::super::attachments::turns::get(db, turn, id).await?;
                        if attachment.spec.media_type != *mime_type {
                            return Err(sailry_protocol::Fault::invalid("attachment media type mismatch"));
                        }
                        Part::Attachment(attachment)
                    }
                    adk_core::Part::Text { text } => Part::Text(text.clone()),
                    adk_core::Part::InlineData { mime_type, data, .. }
                        if mime_type.starts_with("image/") =>
                    {
                        Part::Image(super::images::inline(db, turn, event, index, mime_type, data).await?)
                    }
                    adk_core::Part::Thinking { thinking, .. } => Part::Thinking(thinking.clone()),
                    // Approval and question indices address canonical content parts.
                    // One provider part remains one projected part.
                    adk_core::Part::ServerToolCall { server_tool_call }
                        if server_tool_call["type"] == "message"
                            && server_tool_call["phase"] == "commentary" => {
                        Part::Text(self::commentary(server_tool_call).unwrap_or_default())
                    }
                    adk_core::Part::ServerToolCall { server_tool_call } => {
                        web_search::call(server_tool_call, displays.get("web_search")).map(tools::Raw::part).unwrap_or_else(|| {
                            Part::Resource(
                                serde_json::to_value(part).expect("ADK content is serializable"),
                            )
                        })
                    }
                    adk_core::Part::FunctionCall { .. } => {
                        tools::with_sources(db, turn, sequence, event, index, sources).await?
                            .ok_or_else(|| Fault::invalid("tool call projection is missing"))?.part()
                    },
                    adk_core::Part::ServerToolResponse {
                        server_tool_response,
                    } if server_tool_response["type"] == "anthropic_message" => {
                        Part::Resource(serde_json::json!({"type":"provider_context"}))
                    },
                    adk_core::Part::ServerToolResponse { .. } => tools::with_sources(db, turn, sequence, event, index, sources).await?.map(tools::Raw::part).unwrap_or_else(|| {
                        Part::Resource(
                            serde_json::to_value(part).expect("ADK content is serializable"),
                        )
                    }),
                    adk_core::Part::FunctionResponse { .. } => {
                        tools::with_sources(db, turn, sequence, event, index, sources).await?
                            .ok_or_else(|| Fault::invalid("tool result projection is missing"))?.part()
                    },
                    part => Part::Resource(
                        serde_json::to_value(part).expect("ADK content is serializable"),
                    ),
                })
            })(value).await?); } projected.into_iter().map(Ok::<_,Fault>) }
            .chain(
                event
                    .content()
                    .into_iter()
                    .flat_map(|content| &content.parts)
                    .filter_map(|part| match part {
                        adk_core::Part::ServerToolCall { server_tool_call } => {
                            web_search::result(server_tool_call, displays.get("web_search")).map(|raw| Ok(raw.part()))
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
            .chain(super::elicitation::project(event)?.into_iter().map(|raw| Ok(raw.part())))
            .chain(super::review::project(event)?.into_iter().map(|raw| Ok(raw.part())))
            .chain(failures.into_iter().map(|failure| {
                Ok(Part::Resource(serde_json::json!({
                    "type":"tool_failed", "index":failure.index, "id":failure.id,
                    "name":failure.name, "failure":failure.failure
                })))
            }))
            .chain(event.provider_metadata.get("adk_tool_started").map(|value| {
                let value: Value = serde_json::from_str(value).map_err(storage_error)?;
                let name = value["name"].as_str().ok_or_else(|| sailry_protocol::Fault::invalid("invalid tool start name"))?;
                Ok(Part::Resource(serde_json::json!({"type": "tool_started", "id": value["id"], "name": name})))
            }))
            .chain(event.provider_metadata.get("sailry_compaction_failure").map(|value| {
                if event.author != "system" {
                    return Err(sailry_protocol::Fault::invalid("invalid compaction failure author"));
                }
                let failure: sailry_protocol::conversation::compaction::Failure = serde_json::from_str(value).map_err(storage_error)?;
                Ok(Part::Resource(failure.resource()))
            }))
            .chain(event.provider_metadata.get("sailry_operation_warning").map(|code| {
                if event.author != "system" || code != "context_summary_failed" {
                    return Err(sailry_protocol::Fault::invalid("invalid operation warning metadata"));
                }
                use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};
                Ok(Part::Resource(Failure { source: Source::Automatic, stage: Stage::Unknown, reason: Reason::Unknown, continued: true, status: None, cause: None }.resource()))
            }))
            .chain(event.provider_metadata.get("sailry_retry").map(|value| {
                let retry: sailry_protocol::conversation::recovery::Retry = serde_json::from_str(value).map_err(storage_error)?;
                if event.author != "system" || !retry.valid() {
                    return Err(sailry_protocol::Fault::invalid("invalid model retry metadata"));
                }
                Ok(Part::Resource(retry.resource()))
            }))
            .chain(
                references
                    .into_iter()
                    .map(|reference| Ok(Part::Reference(reference))),
            )
            .collect::<Result<_, Fault>>()?
        },
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
