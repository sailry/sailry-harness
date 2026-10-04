//! Private opaque checkpoints follow visible turn membership, not plugin business rules.
use super::*;
use sailry_protocol::{SessionId, TurnId, plugin::storage::ConversationEntry};
use std::collections::BTreeMap;

pub(super) fn execute(
    db: &Connection,
    request: &Request,
    declaration: Option<&Declaration>,
) -> Result<Output, Fault> {
    let context = request.plugin.as_ref().ok_or_else(denied)?;
    let session = context.session.ok_or_else(denied)?;
    let name = &context.package.name;
    let output = match &request.command {
        Command::ReadPluginConversationValue { key } => {
            let mut entry = read(db, name, session, key)?;
            project_defaults(
                declaration,
                Scope::Conversation,
                key,
                entry.present,
                &mut entry.value,
            )?;
            entry
        }
        Command::WritePluginConversationValue {
            key,
            value,
            expected_revision,
        } => {
            let value = if let Some(schema) = schema(declaration, Scope::Conversation, key) {
                let current = read(db, name, session, key)?;
                check_revision(current.revision, *expected_revision)?;
                crate::plugins::storage::normalize(
                    schema,
                    current.present.then_some(&current.value),
                    value,
                )?
            } else {
                value.clone()
            };
            let turn = anchor(db, session, context.turn)?;
            write(
                db,
                name,
                session,
                turn,
                key,
                Some(&value),
                *expected_revision,
            )?
        }
        Command::RemovePluginConversationValue {
            key,
            expected_revision,
        } => {
            let turn = anchor(db, session, context.turn)?;
            write(db, name, session, turn, key, None, *expected_revision)?
        }
        _ => return Err(invalid("conversation storage command expected")),
    };
    Ok(Output::PluginConversationValue(output))
}

fn anchor(
    db: &Connection,
    session: SessionId,
    turn: Option<TurnId>,
) -> Result<Option<TurnId>, Fault> {
    if let Some(turn) = turn {
        let run = crate::store::agent::visible_run(db, session, turn)?;
        if run.origin.is_some() {
            return Err(denied());
        }
        return Ok(Some(turn));
    }
    db.query_row(
        "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn \
         WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
        [session.to_string()],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(storage_error)?
    .map(|turn| turn.parse().map_err(storage_error))
    .transpose()
}

pub(in crate::store) fn read(
    db: &Connection,
    name: &str,
    session: SessionId,
    key: &str,
) -> Result<ConversationEntry, Fault> {
    check_key(key)?;
    let row: Option<(i64, bool, Option<Vec<u8>>)> = db
        .query_row(
            "SELECT v.revision,v.restored,r.value FROM plugin_conversation_values v \
             LEFT JOIN plugin_conversation_records r ON r.sequence=v.record \
             WHERE v.name=?1 AND v.session=?2 AND v.key=?3",
            params![name, session.to_string(), key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (revision, restored, body) = row.unwrap_or((0, false, None));
    let revision = u64::try_from(revision).map_err(storage_error)?;
    let value = body
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(storage_error)?;
    Ok(ConversationEntry {
        key: key.into(),
        revision,
        present: body.is_some(),
        value: value.unwrap_or(Value::Null),
        restored,
    })
}

fn write(
    db: &Connection,
    name: &str,
    session: SessionId,
    turn: Option<TurnId>,
    key: &str,
    value: Option<&Value>,
    expected: u64,
) -> Result<ConversationEntry, Fault> {
    let current = read(db, name, session, key)?;
    check_revision(current.revision, expected)?;
    if value.is_none() && current.revision == 0 {
        return Ok(current);
    }
    let body = value
        .map(serde_json::to_vec)
        .transpose()
        .map_err(storage_error)?;
    if body
        .as_ref()
        .is_some_and(|body| body.len() > storage::MAX_VALUE_BYTES)
    {
        return Err(invalid("plugin value exceeds 256 KiB"));
    }
    let (count, bytes): (i64, i64) = db
        .query_row(
            "SELECT count(*),coalesce(sum(length(r.value)),0) FROM plugin_conversation_values v \
             JOIN plugin_conversation_records r ON r.sequence=v.record \
             WHERE v.name=?1 AND v.session=?2 AND r.value IS NOT NULL",
            params![name, session.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    let old_bytes = if current.present {
        serde_json::to_vec(&current.value)
            .map_err(storage_error)?
            .len()
    } else {
        0
    };
    let bytes = usize::try_from(bytes)
        .ok()
        .and_then(|bytes| bytes.checked_sub(old_bytes))
        .and_then(|bytes| bytes.checked_add(body.as_ref().map_or(0, Vec::len)))
        .ok_or_else(|| storage_error("invalid plugin storage size"))?;
    let count = count - i64::from(current.present) + i64::from(value.is_some());
    if count > storage::MAX_NAMESPACE_KEYS as i64 || bytes > storage::MAX_NAMESPACE_BYTES {
        return Err(Fault::new(ErrorCode::Busy, "plugin storage is full"));
    }
    let revision = next_revision(current.revision)?;
    let record = record(db, name, key, body.as_deref())?;
    reference(db, session, record, turn)?;
    project(db, name, session, key, revision, Some(record), false)?;
    Ok(ConversationEntry {
        key: key.into(),
        revision: revision as u64,
        value: value.cloned().unwrap_or(Value::Null),
        present: value.is_some(),
        restored: false,
    })
}

fn record(db: &Connection, name: &str, key: &str, body: Option<&[u8]>) -> Result<i64, Fault> {
    db.execute(
        "INSERT INTO plugin_conversation_records(name,key,value) VALUES(?1,?2,?3)",
        params![name, key, body],
    )
    .map_err(storage_error)?;
    Ok(db.last_insert_rowid())
}

fn reference(
    db: &Connection,
    session: SessionId,
    record: i64,
    turn: Option<TurnId>,
) -> Result<(), Fault> {
    db.execute(
        "INSERT OR IGNORE INTO plugin_conversation_history(session,record,turn) VALUES(?1,?2,?3)",
        params![
            session.to_string(),
            record,
            turn.map(|turn| turn.to_string())
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

fn project(
    db: &Connection,
    name: &str,
    session: SessionId,
    key: &str,
    revision: i64,
    record: Option<i64>,
    restored: bool,
) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO plugin_conversation_values(name,session,key,revision,record,restored) \
         VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(name,session,key) DO UPDATE SET \
         revision=excluded.revision,record=excluded.record,restored=excluded.restored",
        params![name, session.to_string(), key, revision, record, restored],
    )
    .map_err(storage_error)?;
    Ok(())
}

/// A transaction's state writes belong to its newly admitted input, including writes before submit.
pub(in crate::store) fn reanchor(
    db: &Connection,
    session: SessionId,
    after: i64,
    turn: TurnId,
) -> Result<(), Fault> {
    db.execute(
        "UPDATE plugin_conversation_history SET turn=?3 WHERE session=?1 AND record>?2",
        params![session.to_string(), after, turn.to_string()],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn sequence(db: &Connection) -> Result<i64, Fault> {
    db.query_row(
        "SELECT coalesce(max(sequence),0) FROM plugin_conversation_records",
        [],
        |row| row.get(0),
    )
    .map_err(storage_error)
}

/// Only references are inherited; later writes and membership changes remain independent.
pub(in crate::store) fn fork(
    db: &Connection,
    source: SessionId,
    target: SessionId,
    through: TurnId,
    preserve_current: bool,
) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO plugin_conversation_history(session,record,turn) \
         SELECT ?2,h.record,h.turn FROM plugin_conversation_history h \
         LEFT JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND \
         (h.turn IS NULL OR t.rowid<=(SELECT rowid FROM turns WHERE id=?3) AND EXISTS \
         (SELECT 1 FROM conversation_turns c WHERE c.session=?1 AND c.turn=h.turn))",
        params![source.to_string(), target.to_string(), through.to_string()],
    )
    .map_err(storage_error)?;
    if preserve_current {
        let mut query = db
            .prepare("SELECT name,key,record FROM plugin_conversation_values WHERE session=?1")
            .map_err(storage_error)?;
        let rows = query
            .query_map([source.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (name, key, checkpoint) = row.map_err(storage_error)?;
            let checkpoint = match checkpoint {
                Some(record) => record,
                None => record(db, &name, &key, None)?,
            };
            reference(db, target, checkpoint, Some(through))?;
        }
    }
    restore(db, target)
}

pub(in crate::store) fn rewind(
    db: &Connection,
    session: SessionId,
    through: Option<TurnId>,
) -> Result<(), Fault> {
    if let Some(through) = through {
        db.execute(
            "DELETE FROM plugin_conversation_history WHERE session=?1 AND turn IN \
             (SELECT id FROM turns WHERE rowid>(SELECT rowid FROM turns WHERE id=?2))",
            params![session.to_string(), through.to_string()],
        )
        .map_err(storage_error)?;
    } else {
        db.execute(
            "DELETE FROM plugin_conversation_history WHERE session=?1",
            [session.to_string()],
        )
        .map_err(storage_error)?;
    }
    restore(db, session)
}

fn restore(db: &Connection, session: SessionId) -> Result<(), Fault> {
    let mut entries = BTreeMap::new();
    {
        let mut query = db
            .prepare("SELECT name,key,revision FROM plugin_conversation_values WHERE session=?1")
            .map_err(storage_error)?;
        for row in query
            .query_map([session.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(storage_error)?
        {
            let (name, key, revision) = row.map_err(storage_error)?;
            entries.insert(
                (name, key),
                (u64::try_from(revision).map_err(storage_error)?, None),
            );
        }
    }
    {
        let mut query = db
            .prepare(
                "SELECT r.name,r.key,max(h.record) FROM plugin_conversation_history h \
             JOIN plugin_conversation_records r ON r.sequence=h.record \
             WHERE h.session=?1 GROUP BY r.name,r.key",
            )
            .map_err(storage_error)?;
        for row in query
            .query_map([session.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(storage_error)?
        {
            let (name, key, record) = row.map_err(storage_error)?;
            entries.entry((name, key)).or_insert((0, None)).1 = Some(record);
        }
    }
    for ((name, key), (revision, record)) in entries {
        project(
            db,
            &name,
            session,
            &key,
            next_revision(revision)?,
            record,
            true,
        )?;
    }
    Ok(())
}

fn denied() -> Fault {
    Fault::new(
        ErrorCode::PermissionDenied,
        "plugin conversation scope is required",
    )
}

#[cfg(test)]
mod tests;
