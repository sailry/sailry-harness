use super::*;
use crate::agent::canonical::{self as records, BlockKind, Kind, Record};
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
// Visibility effects keep messages immutable and are scoped to this conversation's membership.
pub(super) const VISIBLE_EVENT: &str = "CASE WHEN json_extract(e.body,'$.kind')='compaction' THEN 1 ELSE coalesce((SELECT json_extract(v.body,'$.user_visible')
       FROM conversation_events visibility JOIN agent_events v ON v.sequence=visibility.sequence
       WHERE visibility.session=h.session AND json_extract(v.body,'$.kind')='operation'
         AND json_extract(v.body,'$.operation')='visibility'
         AND json_extract(v.body,'$.message')=json_extract(e.body,'$.id')
       ORDER BY visibility.sequence DESC LIMIT 1),
       json_extract(e.body,'$.metadata.runtime.user_visible'),1) END";
#[cfg(test)]
mod tests;
mod web_search;
const APPLICATION_STATE: &str = "app:";
const USER_STATE: &str = "user:";
const TEMP_STATE: &str = "temp:";

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
    event: Record,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    append_with_context(database, session, turn, event, None, events).await
}

pub(super) async fn append_with_context(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    event: Record,
    context: Option<(TurnId, Record)>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    write(
        database,
        session,
        turn,
        Input::Record(event),
        context,
        events,
    )
    .await
}

#[derive(Clone)]
enum Input {
    Record(Record),
    Summary(
        goose_provider_types::conversation::message::Message,
        goose_provider_types::conversation::token_usage::ProviderUsage,
        String,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        Vec<records::CompactedFile>,
    ),
    Message(
        goose_provider_types::conversation::message::Message,
        Option<goose_provider_types::conversation::token_usage::ProviderUsage>,
        BTreeMap<String, String>,
        records::Presentation,
    ),
}

pub(super) async fn append_summary(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    message: goose_provider_types::conversation::message::Message,
    usage: goose_provider_types::conversation::token_usage::ProviderUsage,
    boundary: String,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
    files: Vec<records::CompactedFile>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    write(
        database,
        session,
        turn,
        Input::Summary(message, usage, boundary, start, end, files),
        None,
        events,
    )
    .await
}

pub(super) async fn append_message(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    message: goose_provider_types::conversation::message::Message,
    usage: Option<goose_provider_types::conversation::token_usage::ProviderUsage>,
    calls: BTreeMap<String, String>,
    presentation: records::Presentation,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    write(
        database,
        session,
        turn,
        Input::Message(message, usage, calls, presentation),
        None,
        events,
    )
    .await
}

async fn write(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    event: Input,
    context: Option<(TurnId, Record)>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let permit = database.write().await?;
    for attempt in 0..=crate::store::database::conflict::RETRIES {
        let event = event.clone();
        let context = context.clone();
        let write = async || -> Result<(), Fault> {
            let publication = database.publication.clone();
            let transaction = crate::store::database::conflict::begin(&database.connection).await?;
            let prepared: Result<_, Fault> = async {
                if matches!(&event, Input::Record(record) if record.context().is_some_and(|context| context.captured))
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
                let event = match event {
                    Input::Record(record) => record,
                    Input::Summary(message,usage,boundary,start,end,files)=>{
                        let mut record=crate::agent::engine::history::capture(&transaction,turn,&message,&BTreeMap::new()).await?;
                        capture_usage(&mut record,&usage)?;
                        let records::Kind::Message(summary)=record.kind else{unreachable!()};
                        record.kind=records::Kind::Compaction(records::Compaction{boundary,summary,start,end,files});
                        record.provenance.author="system".into();
                        record
                    },
                    Input::Message(message, usage, calls, presentation) => {
                        let mut record = crate::agent::engine::history::capture(&transaction, turn, &message, &calls).await?;
                        if let Some(usage)=usage {capture_usage(&mut record,&usage)?;}
                        super::presentation::capture_failures(&mut record,None);
                        super::presentation::stamp(&mut record,&presentation);
                        record
                    }
                };
                let previous_visibility = match &event.kind {
                    Kind::Operation(records::Operation::Visibility { message, .. }) => {
                        visibility(&transaction, session, message).await?
                    }
                    _ => None,
                };
                let Some(entry) = insert(&transaction, session, turn, event).await? else {
                    return Ok(None);
                };
                let visible: bool = {
                    let mut statement = transaction
                        .prepare(&format!(
                            "SELECT {VISIBLE_EVENT} FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE e.sequence=?1 AND h.session=?2"
                        ))
                        .await
                        .map_err(storage_error)?;
                    let row = statement
                        .query_row(params![i64::try_from(entry.sequence).map_err(storage_error)?,session.to_string()])
                        .await
                        .map_err(storage_error)?;
                    let mapped: turso::Result<_> = row.get(0);
                    mapped.map_err(storage_error)?
                };
                let (revealed, reset) = if let Some((target, before)) = previous_visibility {
                    let (_, after) = visibility(&transaction, session, &target.id).await?
                        .ok_or_else(|| Fault::invalid("visibility target is missing"))?;
                    if before == after {
                        (None, false)
                    } else if after {
                        let mut query = transaction.prepare(&format!(
                            "SELECT max(e.sequence) FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
                             WHERE h.session=?1 AND e.sequence!=?2 AND {VISIBLE_EVENT}"
                        )).await.map_err(storage_error)?;
                        let row=query.query_row(params![session.to_string(),target.sequence as i64]).await.map_err(storage_error)?;
                        let latest:Option<i64>=row.get(0).map_err(storage_error)?;
                        if latest.is_none_or(|sequence|sequence < target.sequence as i64) {
                            (Some(target), false)
                        } else {
                            (None, true)
                        }
                    } else {
                        (None, true)
                    }
                } else {
                    (None, false)
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
                    entry, visible, revealed, reset, context, background, statistics, parents, envelope, notices,
                )))
            }
            .await;
            let Some((
                entry,
                visible,
                revealed,
                reset,
                context,
                background,
                statistics,
                parents,
                envelope,
                notices,
            )) = prepared?
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
            if reset {
                database.feeds.reset(session);
            } else if let Some(entry) = revealed {
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
            return Err(error);
        }
        if !crate::store::database::conflict::again(&result, attempt).await {
            return result;
        }
    }
    unreachable!("the final attempt returns its result")
}

fn capture_usage(
    record: &mut Record,
    usage: &goose_provider_types::conversation::token_usage::ProviderUsage,
) -> Result<(), Fault> {
    let Some(message) = record.message_mut() else {
        return Ok(());
    };
    message.model = Some(usage.model.clone());
    message.usage = Some(crate::agent::engine::history::provider_usage(usage)?);
    Ok(())
}

async fn visibility(
    db: &Connection,
    session: SessionId,
    id: &str,
) -> Result<Option<(Entry, bool)>, Fault> {
    let mut query = db
        .prepare(&format!(
            "SELECT e.sequence,e.turn,e.body,{VISIBLE_EVENT} FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2"
        ))
        .await
        .map_err(storage_error)?;
    let row = match query.query_row(params![session.to_string(), id]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let sequence: i64 = row.get(0).map_err(storage_error)?;
    let turn: String = row.get(1).map_err(storage_error)?;
    let body: Vec<u8> = row.get(2).map_err(storage_error)?;
    let visible: bool = row.get(3).map_err(storage_error)?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    Ok(Some((
        project(
            db,
            sequence as u64,
            turn.parse().map_err(storage_error)?,
            &record,
        )
        .await?,
        visible,
    )))
}

/// Insert into the caller's existing transaction; this is the same history owner.
pub(super) async fn insert(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    mut event: Record,
) -> Result<Option<Entry>, Fault> {
    if event.id.is_empty() || event.id.len() > 256 {
        return Err(Fault::invalid("event identifier is invalid"));
    }
    if event.turn != turn {
        return Err(Fault::invalid("event belongs to another turn"));
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
    if previous.is_none() && event.provenance.author == "user" {
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
    if let Some(request) = event
        .metadata
        .execution
        .as_mut()
        .and_then(|execution| execution.observation.as_mut())
    {
        let first: Option<String> = {
            let mut statement = db.prepare("SELECT json_extract(e.body,'$.metadata.execution.observation') FROM conversation_events h
             JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1
             AND json_extract(e.body,'$.metadata.execution.observation.id')=?2 ORDER BY h.sequence LIMIT 1").await.map_err(storage_error)?;
            match statement
                .query_row(params![session.to_string(), request.id.as_str()])
                .await
            {
                Ok(row) => Some(row.get(0).map_err(storage_error)?),
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        request.boundary = if let Some(first) = first {
            serde_json::from_str::<sailry_protocol::conversation::observation::Request>(&first)
                .map_err(storage_error)?
                .boundary
        } else {
            let mut statement = db
                .prepare("SELECT max(sequence) FROM conversation_events WHERE session=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([session.to_string()])
                .await
                .map_err(storage_error)?;
            row.get::<Option<i64>>(0)
                .map_err(storage_error)?
                .map(u64::try_from)
                .transpose()
                .map_err(storage_error)?
        };
    }
    records::validate(&event)?;
    super::blobs::validate(db, &event).await?;
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
    if event.provenance.author == "user" {
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

fn changes(event: &Record) -> HashMap<String, Value> {
    let mut delta = event
        .context()
        .map(records::Context::state)
        .unwrap_or_default();
    if let Some(state) = &event.metadata.state {
        delta.extend(
            state
                .session
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        for (prefix, values) in [
            (APPLICATION_STATE, &state.application),
            (USER_STATE, &state.user),
        ] {
            delta.extend(
                values
                    .iter()
                    .map(|(key, value)| (format!("{prefix}{key}"), value.clone())),
            );
        }
    }
    delta
}

async fn apply_state(db: &Connection, session: SessionId, event: &Record) -> Result<(), Fault> {
    let context = event.context();
    let delta = changes(event);
    for (key, value) in &delta {
        if key.starts_with(TEMP_STATE) || key == LOADED_TOOLS {
            continue;
        }
        let owner = if key.starts_with(APPLICATION_STATE) || key.starts_with(USER_STATE) {
            String::new()
        } else {
            session.to_string()
        };
        db.execute("INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", params![owner, key.as_str(), encode(value)?]).await.map_err(storage_error)?;
    }
    let mut names = context
        .and_then(|context| context.loaded_tools.clone())
        .unwrap_or_default();
    names.extend(
        event
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| match &block.kind {
                BlockKind::ToolRequest(request) => Some(request.name.clone()),
                _ => None,
            }),
    );
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
         JOIN json_each(e.body,'$.content') part
         WHERE json_extract(part.value,'$.type')='tool_request'",
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
        JOIN json_each(e.body,'$.content') part WHERE h.session=?1 AND h.sequence<=?2
        AND json_extract(part.value,'$.type')='tool_request'",
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
            "SELECT e.body FROM conversation_events h
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
            let row: turso::Result<_> = row.get::<Vec<u8>>(0);

            let event: Record =
                serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
            for (key, value) in changes(&event) {
                // Shared app/user state belongs to the Node, not a historical session branch.
                if (named && key == SESSION_TITLE)
                    || key.starts_with(TEMP_STATE)
                    || key.starts_with(APPLICATION_STATE)
                    || key.starts_with(USER_STATE)
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
            let mut event: Record = serde_json::from_slice(&body).map_err(storage_error)?;
            if let Some(context) = event.metadata.context.as_mut() {
                context.title = None;
            }
            if let Kind::Configuration(configuration) = &mut event.kind {
                configuration.context.title = None;
            }
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
    event: &Record,
) -> Result<Entry, Fault> {
    project_with_sources(db, sequence, turn, event, None).await
}

async fn project_with_sources(
    db: &Connection,
    sequence: u64,
    turn: TurnId,
    event: &Record,
    sources: Option<&tools::Sources>,
) -> Result<Entry, Fault> {
    let references = super::references::project(event)?;
    let displays = super::presentation::displays(event)?;
    let failures = super::presentation::failures(event)?;
    let input = event.metadata.input.as_ref();
    let skipped = usize::from(!references.is_empty())
        + usize::from(input.is_some_and(|input| input.notice_part.is_some()));
    let hidden = event.provenance.author == "system"
        && event.context().is_some_and(|context| context.captured);
    let capability_part = input.and_then(|input| input.capability_part);
    let mut commentary = BTreeMap::new();
    let mut parts = Vec::new();
    if !hidden {
        if let Some(message) = event.message() {
            for index in skipped..message.part_count {
                if Some(index) == capability_part {
                    continue;
                }
                let Some(block) = message.content.iter().find(|block| block.index == index) else {
                    // Engine-only content leaves its public slot without exposing its payload.
                    parts.push(Part::Resource(
                        serde_json::json!({"type":"provider_context"}),
                    ));
                    continue;
                };
                let part = match &block.kind {
                    BlockKind::Text { text } => Part::Text(text.clone()),
                    BlockKind::Image { asset } => {
                        Part::Image(super::images::inline(db, turn, event, index, asset).await?)
                    }
                    BlockKind::Document { asset } => {
                        let mut value = serde_json::json!({"mime_type":asset.mime,"data":super::blobs::get(db, &asset.digest).await?});
                        if let Some(uri) = &asset.uri {
                            value["uri"] = uri.clone().into();
                        }
                        if let Some(annotations) = &asset.annotations {
                            value["annotations"] = annotations.clone();
                        }
                        Part::Resource(value)
                    }
                    BlockKind::File { file }
                        if crate::agent::attachments::is_reference(&file.uri) =>
                    {
                        let id = crate::agent::attachments::id(&file.uri)?;
                        let attachment =
                            super::super::attachments::turns::get(db, turn, id).await?;
                        if file.mime.as_deref() != Some(attachment.spec.media_type.as_str()) {
                            return Err(Fault::invalid("attachment media type mismatch"));
                        }
                        Part::Attachment(attachment)
                    }
                    BlockKind::File { file } => {
                        let mut value =
                            serde_json::json!({"file_uri":file.uri,"mime_type":file.mime});
                        if let Some(annotations) = &file.annotations {
                            value["annotations"] = annotations.clone();
                        }
                        Part::Resource(value)
                    }
                    BlockKind::Thinking { text, .. } => Part::Thinking(text.clone()),
                    BlockKind::RedactedThinking { .. } => {
                        Part::Resource(serde_json::json!({"type":"provider_context"}))
                    }
                    BlockKind::ServerCall { value } => {
                        let value = super::blobs::hydrate(db, value).await?;
                        if let Some(text) = self::commentary(&value) {
                            commentary.insert(
                                index - skipped,
                                value["id"].as_str().unwrap_or(&event.id).to_owned(),
                            );
                            Part::Text(text)
                        } else {
                            web_search::call(&value, displays.get("web_search"))
                                .map(tools::Raw::part)
                                .unwrap_or_else(|| {
                                    Part::Resource(serde_json::json!({"server_tool_call":value}))
                                })
                        }
                    }
                    BlockKind::ToolRequest(_) => {
                        tools::with_sources(db, turn, sequence, event, index, sources)
                            .await?
                            .ok_or_else(|| Fault::invalid("tool call projection is missing"))?
                            .part()
                    }
                    BlockKind::ToolResponse(_) => {
                        tools::with_sources(db, turn, sequence, event, index, sources)
                            .await?
                            .ok_or_else(|| Fault::invalid("tool result projection is missing"))?
                            .part()
                    }
                    BlockKind::ServerResponse { value } => {
                        let value = super::blobs::hydrate(db, value).await?;
                        if value["type"] == "anthropic_message" {
                            Part::Resource(serde_json::json!({"type":"provider_context"}))
                        } else {
                            tools::with_sources(db, turn, sequence, event, index, sources)
                                .await?
                                .map(tools::Raw::part)
                                .unwrap_or_else(|| {
                                    Part::Resource(
                                        serde_json::json!({"server_tool_response":value}),
                                    )
                                })
                        }
                    }
                    BlockKind::Resource { value } => Part::Resource(
                        serde_json::json!({"resource":super::blobs::hydrate(db, value).await?}),
                    ),
                };
                parts.push(part);
            }
            for block in &message.content {
                if let BlockKind::ServerCall { value } = &block.kind {
                    let value = super::blobs::hydrate(db, value).await?;
                    if let Some(raw) = web_search::result(&value, displays.get("web_search")) {
                        parts.push(raw.part());
                    }
                }
            }
        }
        if let Kind::Compaction(summary) = &event.kind {
            parts.push(Part::Compaction(
                summary
                    .summary
                    .content
                    .iter()
                    .filter_map(|block| match &block.kind {
                        BlockKind::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
        parts.extend(
            super::elicitation::project(event)?
                .into_iter()
                .map(tools::Raw::part),
        );
        parts.extend(
            super::review::project(event)?
                .into_iter()
                .map(tools::Raw::part),
        );
        parts.extend(failures.into_iter().map(|failure| Part::Resource(serde_json::json!({"type":"tool_failed","index":failure.index,"id":failure.id,"name":failure.name,"failure":failure.failure}))));
        if let Some(started) = event
            .metadata
            .execution
            .as_ref()
            .and_then(|execution| execution.tool_started.as_ref())
        {
            parts.push(Part::Resource(
                serde_json::json!({"type":"tool_started","id":started.id,"name":started.name}),
            ));
        }
        if let Some(failure) = event
            .metadata
            .compaction
            .as_ref()
            .and_then(|compaction| compaction.failure.as_ref())
        {
            if event.provenance.author != "system" {
                return Err(Fault::invalid("invalid compaction failure author"));
            }
            parts.push(Part::Resource(failure.resource()));
        }
        if let Some(retry) = event
            .metadata
            .execution
            .as_ref()
            .and_then(|execution| execution.retry.as_ref())
        {
            if event.provenance.author != "system" || !retry.valid() {
                return Err(Fault::invalid("invalid model retry metadata"));
            }
            parts.push(Part::Resource(retry.resource()));
        }
        parts.extend(references.into_iter().map(Part::Reference));
    }
    Ok(Entry {
        sequence,
        turn,
        id: event.id.clone(),
        author: event.provenance.author.clone(),
        branch: event.provenance.branch.clone(),
        timestamp_ms: event.created.timestamp_millis(),
        commentary,
        citations: event.message().map(citations::project).unwrap_or_default(),
        search_suggestions: event
            .message()
            .and_then(|message| message.sources.search_suggestions.as_ref())
            .filter(|html| !html.is_empty())
            .cloned(),
        parts,
        usage: event
            .message()
            .and_then(|message| message.usage.as_ref())
            .map(|usage| Usage {
                input: usage.input.unwrap_or_default(),
                output: usage.output.unwrap_or_default(),
                cached_input: usage.cache_read.unwrap_or_default(),
                reasoning: usage.reasoning.unwrap_or_default(),
            }),
    })
}

pub(super) const EVENTS_QUERY: &str =
    "SELECT e.sequence,e.turn,e.body,t.rowid FROM conversation_events h
    JOIN agent_events e ON e.sequence=h.sequence JOIN turns t ON t.id=e.turn
    WHERE h.session=?1 AND h.sequence<?2 ORDER BY h.sequence DESC LIMIT ?3";
