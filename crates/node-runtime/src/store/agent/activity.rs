use super::*;
use sailry_protocol::activity::{Summary, Waiting};

pub(in crate::store) async fn catalog(
    db: &Connection,
    node: NodeId,
) -> Result<sailry_protocol::activity::Catalog, Fault> {
    let cursor = {
        let mut statement = db
            .prepare("SELECT coalesce(max(cursor),0) FROM events")
            .await
            .map_err(storage_error)?;
        let row = statement.query_row(()).await.map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get::<i64>(0);
        mapped.map_err(storage_error)?
    };
    let catalog = sailry_protocol::activity::Catalog {
        node,
        cursor: cursor.try_into().map_err(storage_error)?,
        projects: super::super::projects::list(db).await?,
        worktrees: super::super::worktrees::list(db).await?,
        sessions: {
            let mut selected = Vec::new();
            for value in super::super::sessions::ordered(db).await?.into_iter() {
                selected.push(
                    async {
                        let id = value;
                        super::super::commands::session(db, id).await
                    }
                    .await?,
                );
            }
            Ok::<_, Fault>(selected)
        }?,
        terminals: super::super::terminals::list(db).await?,
    };
    if serde_json::to_vec(&catalog).map_err(storage_error)?.len()
        > sailry_protocol::activity::MAX_CATALOG_BYTES
    {
        return Err(sailry_protocol::Fault::invalid(
            "activity catalog exceeds the response limit",
        ));
    }
    Ok(catalog)
}

pub(in crate::store) async fn read(db: &Connection, session: SessionId) -> Result<Summary, Fault> {
    let attention = super::super::sessions::attention::read(db, session).await?;
    // Preserve the finishing run until the next queued turn starts. Otherwise
    // queue admission would hide its outcome from Node subscribers.
    let body: Option<Vec<u8>> = {
        let mut statement = db.prepare("SELECT a.body FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
         WHERE h.session=?1 AND json_extract(a.body,'$.joined') IS NULL ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,(json_extract(a.body,'$.status')='queued'),t.rowid DESC LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some(body) = body else {
        return Ok(Summary {
            title: title(db, session).await?,
            attention,
            ..Summary::default()
        });
    };
    let run = runs::project(
        serde_json::from_slice(&body).map_err(storage_error)?,
        session,
    );
    let title = title(db, session).await?;
    let waiting = if matches!(run.status, Status::Running | Status::Stopping) {
        let (approval, question): (bool, bool) = {
            let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='pending'), EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending')").await.map_err(storage_error)?;
            let row = statement
                .query_row([run.turn.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
            mapped.map_err(storage_error)?
        };
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
    let queued = {
        let mut statement = db.prepare("SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1").await.map_err(storage_error)?;
        let row = statement
            .query_row([session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    Ok(Summary {
        title,
        attention,
        run: Some(run),
        waiting,
        queued,
    })
}

pub(in crate::store) async fn previews(
    db: &Connection,
    sessions: &[SessionId],
) -> Result<Vec<sailry_protocol::activity::Preview>, Fault> {
    if sessions.len() > 32 {
        return Err(sailry_protocol::Fault::invalid(
            "activity preview batch exceeds 32 sessions",
        ));
    }
    {
        let mut selected = Vec::new();
        for value in sessions.iter() {
            selected.push(
                (async |session: &SessionId| {
                    check_session(db, *session).await?;
                    let summary = read(db, *session).await?;
                    let turn = summary.run.map(|run| run.turn);
                    Ok(sailry_protocol::activity::Preview {
                        session: *session,
                        text: match turn {
                            Some(turn) => preview(db, turn).await?,
                            None => None,
                        },
                        turn,
                    })
                })(value)
                .await?,
            );
        }
        Ok::<_, Fault>(selected)
    }
}

async fn preview(db: &Connection, turn: TurnId) -> Result<Option<String>, Fault> {
    (match (async {
        let mut statement = db
            .prepare(
                "SELECT substr(json_extract(part.value,'$.text'),1,512)
         FROM agent_events e, json_each(e.body,'$.content.parts') part
         WHERE e.turn=?1 AND json_extract(e.body,'$.author')!='user'
           AND json_type(part.value,'$.thinking') IS NULL
           AND length(trim(json_extract(part.value,'$.text')))>0
         ORDER BY e.sequence DESC,part.key ASC LIMIT 1",
            )
            .await?;
        let row = statement.query_row([turn.to_string()]).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    })
    .map_err(storage_error)
}

pub(in crate::store) async fn changed(db: &Connection, session: SessionId) -> Result<Event, Fault> {
    Ok(Event::ConversationChanged {
        session,
        activity: Box::new(read(db, session).await?),
    })
}
pub(in crate::store) async fn queued(db: &Connection, turn: QueuedTurn) -> Result<Event, Fault> {
    Ok(Event::TurnQueued {
        activity: Box::new(read(db, turn.session).await?),
        turn,
    })
}

pub(in crate::store) async fn title(db: &Connection, session: SessionId) -> Result<String, Fault> {
    let source = title_source(db, session).await?;
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

pub(super) async fn assigned_title(
    db: &Connection,
    session: SessionId,
) -> Result<Option<String>, Fault> {
    let assigned: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_state WHERE owner=?1 AND key=?2")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![session.to_string(), SESSION_TITLE])
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
    assigned
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

async fn title_source(db: &Connection, session: SessionId) -> Result<String, Fault> {
    if let Some(title) = assigned_title(db, session).await? {
        return Ok(title);
    }
    let first: Option<String> = {
        let mut statement = db.prepare("SELECT t.id FROM conversation_turns h JOIN turns t ON t.id=h.turn
         JOIN agent_runs a ON a.turn=t.id WHERE h.session=?1 AND json_extract(a.body,'$.kind')='task'
         ORDER BY t.rowid LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some(first) = first else {
        return Ok(String::new());
    };
    let message: Option<String> = { let mut statement = db.prepare("SELECT coalesce(json_extract(r.body,'$.command.data.message.text'),json_extract(r.body,'$.command.data.response.data.message.text'))
         FROM turns t LEFT JOIN agent_pending p ON p.turn=t.id JOIN requests r ON r.id=coalesce(p.request,t.request) AND r.caller=coalesce(p.caller,t.caller) WHERE t.id=?1").await.map_err(storage_error)?;
match statement.query_row([first.as_str()]).await {
Ok(row) => { let mapped: turso::Result<_> = row.get(0); Some(mapped.map_err(storage_error)?) },
Err(turso::Error::QueryReturnedNoRows) => None,
Err(error) => return Err(storage_error(error)),
} }.flatten();
    match message {
        Some(message) => Ok(message),
        None => (match (async { let mut statement = db.prepare("SELECT json_extract(e.body,'$.content.parts[0].text') FROM agent_events e WHERE turn=?1 AND json_extract(body,'$.author')='user' ORDER BY sequence LIMIT 1").await?;
let row = statement.query_row([first.as_str()]).await?;
let mapped: turso::Result<_> = row.get::<Option<String>>(0);
mapped }).await { Ok(value) => Ok(Some(value)), Err(turso::Error::QueryReturnedNoRows) => Ok(None), Err(error) => Err(error) }).map_err(storage_error).map(|message| message.flatten().unwrap_or_default()),
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
