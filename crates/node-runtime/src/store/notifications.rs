//! The persistent source for notices, reduced by the shared Client feed.
pub(super) mod diagnostics;
pub(super) mod system;
#[cfg(test)]
mod tests;
use super::database::{
    counters::{self, Namespace},
    encode, storage_error,
};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, NotificationId, Output,
    notification::{LIMIT, Notice},
};
use turso::{Connection, params};

pub(super) async fn execute(
    db: &Connection,
    command: &Command,
    admitted: Option<sailry_protocol::plugin::Info>,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::PublishNotification { package, content } => {
            let info = if let Some(info) = admitted {
                info
            } else {
                super::plugins::get(db, &package.name)
                    .await?
                    .ok_or_else(|| {
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
                return Err(Fault::invalid("invalid notification content"));
            }
            if content.presentation.as_ref().is_some_and(|presentation| {
                presentation.heading.trim().is_empty()
                    || presentation.heading.len() > 128
                    || presentation.locales.len() > 32
                    || presentation.locales.iter().any(|(locale, heading)| {
                        locale.is_empty()
                            || locale.len() > 32
                            || !locale
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                            || heading.trim().is_empty()
                            || heading.len() > 128
                    })
            }) {
                return Err(Fault::invalid("invalid notification presentation"));
            }
            if let Some(session) = content.session {
                super::commands::session(db, session).await?;
            }
            if content
                .details
                .as_ref()
                .is_some_and(|details| match details {
                    sailry_protocol::notification::Details::Text { body } => {
                        body.trim().is_empty() || body.len() > 8192
                    }
                    _ => true,
                })
            {
                return Err(Fault::invalid("notification details must be bounded text"));
            }
            let notice = insert(
                db,
                package.name.clone(),
                content.clone(),
                chrono::Utc::now().timestamp_millis(),
            )
            .await?;
            Ok((
                Output::Notification(notice.clone()),
                Some(Event::NotificationChanged(notice)),
            ))
        }
        Command::MarkNotificationRead { id } => {
            let mut notice = read(db, *id)
                .await?
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "notification is unavailable"))?;
            if notice.read {
                return Ok((Output::Notification(notice), None));
            }
            notice.read = true;
            put(db, &notice).await?;
            Ok((
                Output::Notification(notice.clone()),
                Some(Event::NotificationChanged(notice)),
            ))
        }
        Command::MarkNotificationsRead { ids } => {
            if ids.len() > LIMIT {
                return Err(Fault::invalid("too many notifications"));
            }
            let mut changed = Vec::new();
            for id in ids {
                if let Some(mut notice) = read(db, *id).await?
                    && !notice.read
                {
                    notice.read = true;
                    put(db, &notice).await?;
                    changed.push(*id);
                }
            }
            Ok((
                Output::NotificationsUpdated,
                (!changed.is_empty()).then_some(Event::NotificationsRead { ids: changed }),
            ))
        }
        Command::DismissReadNotifications { ids } => {
            if ids.len() > LIMIT {
                return Err(Fault::invalid("too many notifications"));
            }
            let mut removed = Vec::new();
            for id in ids {
                if read(db, *id).await?.is_some_and(|notice| notice.read) {
                    db.execute("DELETE FROM notifications WHERE id=?1", [id.to_string()])
                        .await
                        .map_err(storage_error)?;
                    removed.push(*id);
                }
            }
            Ok((
                Output::NotificationsUpdated,
                (!removed.is_empty()).then_some(Event::NotificationsDismissed { ids: removed }),
            ))
        }
        Command::DismissNotifications { ids } => {
            if ids.len() > LIMIT {
                return Err(Fault::invalid("too many notifications"));
            }
            for id in ids {
                db.execute("DELETE FROM notifications WHERE id=?1", [id.to_string()])
                    .await
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

async fn insert(
    db: &Connection,
    package: String,
    content: sailry_protocol::notification::Draft,
    timestamp_ms: i64,
) -> Result<Notice, Fault> {
    let notice = Notice {
        id: NotificationId::new(),
        sequence: counters::next(db, Namespace::Notifications).await?,
        package,
        content,
        timestamp_ms,
        read: false,
    };
    db.execute(
        "INSERT INTO notifications(sequence,id,body) VALUES(?1,?2,?3)",
        params![
            notice.sequence as i64,
            notice.id.to_string(),
            encode(&notice)?
        ],
    )
    .await
    .map_err(storage_error)?;
    db.execute("DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT ?1)", [LIMIT as i64]).await.map_err(storage_error)?;
    Ok(notice)
}

async fn put(db: &Connection, notice: &Notice) -> Result<(), Fault> {
    db.execute(
        "UPDATE notifications SET body=?2 WHERE id=?1",
        params![notice.id.to_string(), encode(notice)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn read(db: &Connection, id: NotificationId) -> Result<Option<Notice>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM notifications WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) async fn list(db: &Connection) -> Result<Vec<Notice>, Fault> {
    let mut rows = db
        .query(
            "SELECT body FROM notifications ORDER BY sequence DESC LIMIT ?1",
            [LIMIT as i64],
        )
        .await
        .map_err(storage_error)?;
    let mut notices = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body: Vec<u8> = row.get(0).map_err(storage_error)?;
        notices.push(serde_json::from_slice(&body).map_err(storage_error)?);
    }
    Ok(notices)
}
