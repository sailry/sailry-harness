use super::*;
use sailry_protocol::activity::{Summary, Waiting};

pub(in crate::store) fn catalog(
    db: &Connection,
    node: NodeId,
) -> Result<sailry_protocol::activity::Catalog, Fault> {
    let cursor = db
        .query_row("SELECT coalesce(max(cursor),0) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(storage_error)?;
    let catalog = sailry_protocol::activity::Catalog {
        node,
        cursor: cursor.try_into().map_err(storage_error)?,
        projects: super::super::projects::list(db)?,
        worktrees: super::super::worktrees::list(db)?,
        sessions: super::super::sessions::ordered(db)?
            .into_iter()
            .map(|id| super::super::commands::session(db, id))
            .collect::<Result<_, _>>()?,
        terminals: super::super::terminals::list(db)?,
    };
    if serde_json::to_vec(&catalog).map_err(storage_error)?.len()
        > sailry_protocol::activity::MAX_CATALOG_BYTES
    {
        return Err(invalid("activity catalog exceeds the response limit"));
    }
    Ok(catalog)
}

pub(in crate::store) fn read(db: &Connection, session: SessionId) -> Result<Summary, Fault> {
    let attention = super::super::sessions::attention::read(db, session)?;
    // Preserve the finishing run until the next queued turn starts. Otherwise
    // queue admission would hide its outcome from Node subscribers.
    let body: Option<Vec<u8>> = db.query_row(
        "SELECT a.body FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
         WHERE h.session=?1 ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,(json_extract(a.body,'$.status')='queued'),t.rowid DESC LIMIT 1",
        [session.to_string()], |row| row.get(0),
    ).optional().map_err(storage_error)?;
    let Some(body) = body else {
        return Ok(Summary {
            title: title(db, session)?,
            attention,
            ..Summary::default()
        });
    };
    let run = runs::project(
        serde_json::from_slice(&body).map_err(storage_error)?,
        session,
    );
    let title = title(db, session)?;
    let waiting = if matches!(run.status, Status::Running | Status::Stopping) {
        let (approval, question): (bool, bool) = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='pending'), EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending')",
            [run.turn.to_string()], |row| Ok((row.get(0)?,row.get(1)?)),
        ).map_err(storage_error)?;
        if approval {
            Some(Waiting::Approval)
        } else if question {
            Some(Waiting::Input)
        } else {
            None
        }
    } else {
        None
    };
    let queued = db
        .query_row(
            "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    Ok(Summary {
        title,
        attention,
        run: Some(run),
        waiting,
        queued,
    })
}

pub(in crate::store) fn previews(
    db: &Connection,
    sessions: &[SessionId],
) -> Result<Vec<sailry_protocol::activity::Preview>, Fault> {
    if sessions.len() > 32 {
        return Err(invalid("activity preview batch exceeds 32 sessions"));
    }
    sessions
        .iter()
        .map(|session| {
            check_session(db, *session)?;
            let summary = read(db, *session)?;
            let turn = summary.run.map(|run| run.turn);
            Ok(sailry_protocol::activity::Preview {
                session: *session,
                text: turn.map(|turn| preview(db, turn)).transpose()?.flatten(),
                turn,
            })
        })
        .collect()
}

fn preview(db: &Connection, turn: TurnId) -> Result<Option<String>, Fault> {
    db.query_row(
        "SELECT substr(json_extract(part.value,'$.text'),1,512)
         FROM agent_events e, json_each(e.body,'$.content.parts') part
         WHERE e.turn=?1 AND json_extract(e.body,'$.author')!='user'
           AND json_type(part.value,'$.thinking') IS NULL
           AND length(trim(json_extract(part.value,'$.text')))>0
         ORDER BY e.sequence DESC,part.key ASC LIMIT 1",
        [turn.to_string()],
        |row| row.get(0),
    )
    .optional()
    .map_err(storage_error)
}

pub(in crate::store) fn changed(db: &Connection, session: SessionId) -> Result<Event, Fault> {
    Ok(Event::ConversationChanged {
        session,
        activity: read(db, session)?,
    })
}
pub(in crate::store) fn queued(db: &Connection, turn: QueuedTurn) -> Result<Event, Fault> {
    Ok(Event::TurnQueued {
        activity: read(db, turn.session)?,
        turn,
    })
}

pub(in crate::store) fn title(db: &Connection, session: SessionId) -> Result<String, Fault> {
    let source = title_source(db, session)?;
    Ok(short_title(&source))
}

fn short_title(source: &str) -> String {
    const LIMIT: usize = 48;
    let title: String = source
        .split_whitespace()
        .flat_map(|word| std::iter::once(' ').chain(word.chars()))
        .skip(1)
        .filter(|character| !character.is_control())
        .take(LIMIT + 1)
        .collect();
    if title.chars().count() <= LIMIT {
        title
    } else {
        let prefix: String = title.chars().take(LIMIT - 1).collect();
        format!("{}…", prefix.trim_end())
    }
}

fn title_source(db: &Connection, session: SessionId) -> Result<String, Fault> {
    let assigned: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_state WHERE owner=?1 AND key=?2",
            params![session.to_string(), SESSION_TITLE],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(body) = assigned {
        return serde_json::from_slice(&body).map_err(storage_error);
    }
    let first: Option<String> = db.query_row(
        "SELECT t.id FROM conversation_turns h JOIN turns t ON t.id=h.turn
         JOIN agent_runs a ON a.turn=t.id WHERE h.session=?1 AND json_extract(a.body,'$.kind')='task'
         ORDER BY t.rowid LIMIT 1",
        [session.to_string()], |row| row.get(0),
    ).optional().map_err(storage_error)?;
    let Some(first) = first else {
        return Ok(String::new());
    };
    let message: Option<String> = db.query_row(
        "SELECT coalesce(json_extract(r.body,'$.command.data.message.text'),json_extract(r.body,'$.command.data.response.data.message.text'))
         FROM turns t LEFT JOIN agent_pending p ON p.turn=t.id JOIN requests r ON r.id=coalesce(p.request,t.request) AND r.caller=coalesce(p.caller,t.caller) WHERE t.id=?1",
        [&first], |row| row.get(0),
    ).optional().map_err(storage_error)?.flatten();
    match message {
        Some(message) => Ok(message),
        None => db.query_row(
            "SELECT json_extract(e.body,'$.content.parts[0].text') FROM agent_events e WHERE turn=?1 AND json_extract(body,'$.author')='user' ORDER BY sequence LIMIT 1",
            [&first], |row| row.get::<_, Option<String>>(0),
        ).optional().map_err(storage_error).map(|message| message.flatten().unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::short_title;

    #[test]
    fn normalizes_whitespace_and_bounds_unicode() {
        assert_eq!(
            short_title("  inspect\n  files\tand changes  "),
            "inspect files and changes"
        );
        assert_eq!(short_title(&"测".repeat(48)), "测".repeat(48));
        assert_eq!(
            short_title(&"测".repeat(49)),
            format!("{}…", "测".repeat(47))
        );
        assert_eq!(
            short_title(&format!("{}  more", "a".repeat(47))),
            format!("{}…", "a".repeat(47))
        );
        assert_eq!(short_title(" \n\t "), "");
    }
}
