//! System notices are committed with the state transition that produced them.
use super::*;
use sailry_protocol::{
    SessionId,
    conversation::{Entry, Part, Run, Status, compaction::Failure},
    notification::{Cause, Details, Diagnostic, Draft, Kind, Task, Terminal},
};

pub(in crate::store) fn power(db: &Connection, code: i64) -> Result<Event, Fault> {
    let notice = insert(
        db,
        String::new(),
        Draft {
            title: String::new(),
            message: String::new(),
            kind: Kind::Error,
            session: None,
            presentation: None,
            details: Some(Details::Power { code }),
        },
        chrono::Utc::now().timestamp_millis(),
    )?;
    Ok(Event::NotificationChanged(notice))
}

pub(in crate::store) fn compaction(
    db: &Connection,
    session: SessionId,
    entry: &Entry,
) -> Result<Option<Event>, Fault> {
    if entry.author != "system" {
        return Ok(None);
    }
    let failure = entry.parts.iter().find_map(|part| match part {
        Part::Resource(value) => Failure::from_resource(value),
        _ => None,
    });
    let Some(failure) = failure else {
        return Ok(None);
    };
    let notice = insert(
        db,
        String::new(),
        Draft {
            title: String::new(),
            message: String::new(),
            kind: Kind::Error,
            session: Some(session),
            presentation: None,
            details: Some(Details::Diagnostic(Diagnostic {
                event: entry.id.clone(),
                sequence: entry.sequence,
                turn: entry.turn,
                cause: Cause::Compaction(failure),
                version: env!("CARGO_PKG_VERSION").into(),
                platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            })),
        },
        entry.timestamp_ms,
    )?;
    Ok(Some(Event::NotificationChanged(notice)))
}

pub(in crate::store) fn run(db: &Connection, run: &Run) -> Result<Option<Event>, Fault> {
    if !matches!(
        run.status,
        Status::Completed | Status::Failed | Status::Interrupted
    ) {
        return Ok(None);
    }
    let session = super::super::commands::session(db, run.session)?;
    if session.delegation.is_some() {
        return Ok(None);
    }
    // A specific diagnostic already explains this failure. A later success or
    // a different failure in the same turn remains a distinct outcome.
    if run.status != Status::Completed && (stopped(db, run.turn)? || reported(db, run)?) {
        return Ok(None);
    }
    let notice = insert(
        db,
        String::new(),
        Draft {
            title: session.activity.title,
            message: String::new(),
            kind: if run.status == Status::Completed {
                Kind::Success
            } else {
                Kind::Error
            },
            session: Some(run.session),
            presentation: None,
            details: Some(Details::Task(Task {
                turn: run.turn,
                status: run.status,
                started_ms: run.started_ms,
                finished_ms: run.finished_ms,
                error: run.error.as_ref().map(|error| error.code),
            })),
        },
        run.finished_ms
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
    )?;
    Ok(Some(Event::NotificationChanged(notice)))
}

fn reported(db: &Connection, run: &Run) -> Result<bool, Fault> {
    let Some(sailry_protocol::FaultDetails::SystemDiagnostic(report)) =
        run.error.as_ref().and_then(|fault| fault.details.as_ref())
    else {
        return Ok(false);
    };
    Ok(list(db)?.iter().any(|notice| {
        matches!(&notice.content.details, Some(Details::System(existing))
            if existing.incident == report.incident && existing.turn == Some(run.turn))
    }))
}

fn stopped(db: &Connection, turn: sailry_protocol::TurnId) -> Result<bool, Fault> {
    let mut query = db.prepare(
        "SELECT json_extract(body,'$.event_metadata.sailry_compaction_failure') FROM agent_events WHERE turn=?1 AND json_extract(body,'$.author')='system'",
    ).map_err(storage_error)?;
    let rows = query
        .query_map([turn.to_string()], |row| row.get::<_, Option<String>>(0))
        .map_err(storage_error)?;
    for row in rows {
        if row
            .map_err(storage_error)?
            .and_then(|value| serde_json::from_str::<Failure>(&value).ok())
            .is_some_and(|failure| !failure.continued)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in crate::store) fn terminal(
    db: &Connection,
    previous: &sailry_protocol::terminal::Info,
    current: &sailry_protocol::terminal::Info,
) -> Result<Option<Event>, Fault> {
    use sailry_protocol::terminal::Status;
    if previous.status != Status::Running || current.worktree.is_none() {
        return Ok(None);
    }
    let (code, failed) = match current.status {
        Status::Exited { code } => (Some(code), code != 0),
        Status::Failed { .. } => (None, true),
        _ => return Ok(None),
    };
    let notice = insert(
        db,
        String::new(),
        Draft {
            title: current.title.clone().unwrap_or_default(),
            message: String::new(),
            kind: if failed { Kind::Error } else { Kind::Success },
            session: None,
            presentation: None,
            details: Some(Details::Terminal(Terminal {
                id: current.id,
                code,
                failed,
            })),
        },
        chrono::Utc::now().timestamp_millis(),
    )?;
    Ok(Some(Event::NotificationChanged(notice)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        TurnId,
        conversation::compaction::{Reason, Source, Stage},
    };

    #[test]
    fn continued_failures_preserve_later_outcomes() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE agent_events(turn TEXT, body BLOB)")
            .unwrap();
        let turn = TurnId::new();
        let mut failure = Failure {
            source: Source::Automatic,
            stage: Stage::Request,
            reason: Reason::RequestFailed,
            continued: true,
            status: None,
            cause: None,
        };
        for (author, value, expected) in [
            ("system", "invalid".to_owned(), false),
            ("system", serde_json::to_string(&failure).unwrap(), false),
            (
                "model",
                {
                    failure.continued = false;
                    serde_json::to_string(&failure).unwrap()
                },
                false,
            ),
            ("system", serde_json::to_string(&failure).unwrap(), true),
        ] {
            let mut event = adk_core::Event::new(turn.to_string());
            event.author = author.into();
            event
                .provider_metadata
                .insert("sailry_compaction_failure".into(), value);
            db.execute(
                "INSERT INTO agent_events(turn,body) VALUES(?1,?2)",
                params![turn.to_string(), encode(&event).unwrap()],
            )
            .unwrap();
            assert_eq!(stopped(&db, turn).unwrap(), expected);
        }
        assert!(!stopped(&db, TurnId::new()).unwrap());
    }
}
