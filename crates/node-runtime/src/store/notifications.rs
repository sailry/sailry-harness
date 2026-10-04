//! The persistent source for plugin notices, reduced by the shared Client feed.
use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, NotificationId, Output,
    notification::{LIMIT, Notice},
};

pub(super) fn execute(
    db: &Connection,
    command: &Command,
    admitted: Option<sailry_protocol::plugin::Info>,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::PublishNotification { package, content } => {
            let info = if let Some(info) = admitted {
                info
            } else {
                super::plugins::get(db, &package.name)?.ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "notification plugin is unavailable")
                })?
            };
            if info.summary.reference() != *package {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "notification plugin changed",
                ));
            }
            if !info.summary.enabled {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "notification plugin is disabled",
                ));
            }
            if content.title.trim().is_empty()
                || content.title.len() > 512
                || content.message.len() > 4096
            {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid notification content",
                ));
            }
            if let Some(session) = content.session {
                super::commands::session(db, session)?;
            }
            let mut notice = Notice {
                id: NotificationId::new(),
                sequence: 0,
                package: package.name.clone(),
                content: content.clone(),
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                read: false,
            };
            db.execute(
                "INSERT INTO notifications(id,body) VALUES(?1,?2)",
                params![notice.id.to_string(), encode(&notice)?],
            )
            .map_err(storage_error)?;
            notice.sequence = db.last_insert_rowid() as u64;
            put(db, &notice)?;
            db.execute("DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT ?1)", [LIMIT as i64]).map_err(storage_error)?;
            Ok((
                Output::Notification(notice.clone()),
                Some(Event::NotificationChanged(notice)),
            ))
        }
        Command::MarkNotificationRead { id } => {
            let mut notice = read(db, *id)?
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "notification is unavailable"))?;
            if notice.read {
                return Ok((Output::Notification(notice), None));
            }
            notice.read = true;
            put(db, &notice)?;
            Ok((
                Output::Notification(notice.clone()),
                Some(Event::NotificationChanged(notice)),
            ))
        }
        Command::DismissNotifications { ids } => {
            if ids.len() > LIMIT {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "too many notifications",
                ));
            }
            for id in ids {
                db.execute("DELETE FROM notifications WHERE id=?1", [id.to_string()])
                    .map_err(storage_error)?;
            }
            Ok((
                Output::NotificationsDismissed,
                (!ids.is_empty()).then(|| Event::NotificationsDismissed { ids: ids.clone() }),
            ))
        }
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "notification command expected",
        )),
    }
}

fn put(db: &Connection, notice: &Notice) -> Result<(), Fault> {
    db.execute(
        "UPDATE notifications SET body=?2 WHERE id=?1",
        params![notice.id.to_string(), encode(notice)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

fn read(db: &Connection, id: NotificationId) -> Result<Option<Notice>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM notifications WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) fn list(db: &Connection) -> Result<Vec<Notice>, Fault> {
    db.prepare("SELECT body FROM notifications ORDER BY sequence DESC LIMIT ?1")
        .map_err(storage_error)?
        .query_map([LIMIT as i64], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
        .collect()
}
