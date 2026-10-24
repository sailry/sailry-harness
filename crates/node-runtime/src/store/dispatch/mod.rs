//! Durable orchestration records share the Node database and command admission.
use super::{
    commands::check_revision,
    database::{encode, storage_error},
};
use sailry_protocol::{self as protocol, ErrorCode, Fault, dispatch::*, plugin::Reference};
use serde::{Serialize, de::DeserializeOwned};
use turso::{Connection, params};

mod calendar;
mod events;
mod handlers;
pub(crate) mod jobs;
pub(in crate::store) mod native;
mod schedules;
mod worker;
pub(crate) use worker::Batch;
pub(super) use worker::{finish, tick};
#[cfg(test)]
mod tests;

pub(super) async fn settings_changed(db: &Connection, package: &Reference) -> Result<(), Fault> {
    let revision = package.settings_revision.to_string();
    let mut reference = serde_json::to_value(package).map_err(storage_error)?;
    reference["settings_revision"] = serde_json::json!(revision);
    events::publish(
        db,
        &Event {
            id: protocol::EventId::new(),
            source: Source {
                package: package.name.clone(),
                topic: "settings.changed".into(),
            },
            payload: serde_json::json!({"package":reference,"settings_revision":revision}),
            timestamp_ms: now(),
            schedule: None,
            scheduled_ms: None,
        },
    )
    .await?;
    Ok(())
}

pub(super) async fn execute(
    db: &Connection,
    package: &Reference,
    action: &Command,
) -> Result<(protocol::Output, Option<protocol::Event>), Fault> {
    available(db, package).await?;
    execute_admitted(db, package, action).await
}

/// The caller has checked the original running invocation and its declared scope.
pub(super) async fn execute_admitted(
    db: &Connection,
    package: &Reference,
    action: &Command,
) -> Result<(protocol::Output, Option<protocol::Event>), Fault> {
    let mut packages = vec![package.name.clone()];
    let (output, changed) = match action {
        Command::ListHandlers => (
            Output::Handlers(list(db, "dispatch_handlers", &package.name).await?),
            false,
        ),
        Command::SaveHandler(handler) => (
            Output::Handler(handlers::save(db, package, handler).await?),
            true,
        ),
        Command::RemoveHandler {
            name,
            expected_revision,
            cancel_pending,
        } => {
            let old: Handler =
                required(db, "dispatch_handlers", &package.name, "name", name).await?;
            check_revision(old.revision, *expected_revision)?;
            if *cancel_pending {
                jobs::cancel_pending(db, &package.name, name).await?;
            }
            db.execute(
                "DELETE FROM dispatch_handlers WHERE package=?1 AND name=?2",
                params![package.name.as_str(), name.as_str()],
            )
            .await
            .map_err(storage_error)?;
            (Output::Removed, true)
        }
        Command::ListSchedules => (
            Output::Schedules(list(db, "dispatch_schedules", &package.name).await?),
            false,
        ),
        Command::SaveSchedule(schedule) => (
            Output::Schedule(schedules::save(db, &package.name, schedule).await?),
            true,
        ),
        Command::RemoveSchedule {
            id,
            expected_revision,
        } => {
            let old: Schedule = required(
                db,
                "dispatch_schedules",
                &package.name,
                "id",
                &id.to_string(),
            )
            .await?;
            check_revision(old.revision, *expected_revision)?;
            db.execute(
                "DELETE FROM dispatch_schedules WHERE package=?1 AND id=?2",
                params![package.name.as_str(), id.to_string()],
            )
            .await
            .map_err(storage_error)?;
            (Output::Removed, true)
        }
        Command::Publish { topic, payload } => {
            name(topic)?;
            data(payload)?;
            let event = Event {
                id: protocol::EventId::new(),
                source: Source {
                    package: package.name.clone(),
                    topic: topic.clone(),
                },
                payload: payload.clone(),
                timestamp_ms: now(),
                schedule: None,
                scheduled_ms: None,
            };
            let (jobs, owners) = events::publish(db, &event).await?;
            packages = owners;
            (
                Output::Published {
                    event: event.id,
                    jobs,
                },
                true,
            )
        }
        Command::Enqueue { handler, payload } => {
            data(payload)?;
            let handler: Handler =
                required(db, "dispatch_handlers", &package.name, "name", handler).await?;
            if !handler.enabled {
                return Err(sailry_protocol::Fault::invalid("event handler is disabled"));
            }
            let reference = current(db, &package.name).await?.summary.reference();
            available(db, &reference).await?;
            let event = Event {
                id: protocol::EventId::new(),
                source: Source {
                    package: package.name.clone(),
                    topic: handler.source.topic.clone(),
                },
                payload: payload.clone(),
                timestamp_ms: now(),
                schedule: None,
                scheduled_ms: None,
            };
            (
                Output::Job(events::enqueue(db, Some(&reference), &handler, &event).await?),
                true,
            )
        }
        Command::ListJobs { before, limit } => (
            Output::Jobs(jobs::list(db, &package.name, *before, *limit).await?),
            false,
        ),
        Command::ReadJob { id } => (
            Output::Job(required(db, "dispatch_jobs", &package.name, "id", &id.to_string()).await?),
            false,
        ),
        Command::CancelJob { id } => (
            Output::Job(jobs::cancel(db, &package.name, *id).await?),
            true,
        ),
        Command::ReadResult { id } => (
            Output::Result(jobs::result(db, &package.name, *id).await?),
            false,
        ),
    };
    Ok((
        protocol::Output::Dispatch(output),
        changed.then_some(protocol::Event::DispatchChanged { packages }),
    ))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn name(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(sailry_protocol::Fault::invalid(
            "dispatch names must use letters, digits, dots, underscores or hyphens",
        ));
    }
    Ok(())
}

fn data(value: &impl Serialize) -> Result<(), Fault> {
    if encode(value)?.len() > MAX_PAYLOAD_BYTES {
        return Err(sailry_protocol::Fault::invalid(
            "dispatch payload exceeds the size limit",
        ));
    }
    Ok(())
}

pub(super) async fn available(db: &Connection, package: &Reference) -> Result<(), Fault> {
    let installed = current(db, &package.name).await?;
    if installed.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "dispatch plugin changed",
        ));
    }
    Ok(())
}

async fn current(db: &Connection, name: &str) -> Result<protocol::plugin::Info, Fault> {
    let installed = super::plugins::get(db, name)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "dispatch plugin is not installed"))?;
    if !installed.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "dispatch plugin is disabled",
        ));
    }
    Ok(installed)
}

async fn capacity(db: &Connection, table: &str, package: &str) -> Result<(), Fault> {
    let count: i64 = {
        let mut statement = db
            .prepare(&format!("SELECT count(*) FROM {table} WHERE package=?1"))
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([package])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if count >= MAX_ITEMS as i64 {
        return Err(Fault::new(
            ErrorCode::Busy,
            "dispatch configuration capacity exhausted",
        ));
    }
    Ok(())
}

async fn budget(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
    value: &impl Serialize,
) -> Result<(), Fault> {
    let total: i64 =
        {
            let mut statement = db.prepare(&format!(
                "SELECT coalesce(sum(length(body)),0) FROM {table} WHERE package=?1 AND {key}!=?2"
            )).await.map_err(storage_error)?;
            let row = statement
                .query_row(params![package, id])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
    if total as usize + encode(value)?.len() > 4 * 1024 * 1024 {
        return Err(Fault::new(
            ErrorCode::Busy,
            "dispatch namespace capacity exhausted",
        ));
    }
    Ok(())
}

async fn get<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
) -> Result<Option<T>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare(&format!(
                "SELECT body FROM {table} WHERE package=?1 AND {key}=?2"
            ))
            .await
            .map_err(storage_error)?;
        match statement.query_row(params![package, id]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

async fn required<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
) -> Result<T, Fault> {
    get(db, table, package, key, id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "dispatch resource is unavailable"))
}

async fn list<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    package: &str,
) -> Result<Vec<T>, Fault> {
    let mut query = db
        .prepare(&format!(
            "SELECT body FROM {table} WHERE package=?1 ORDER BY rowid"
        ))
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query.query([package]).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
    .collect()
}
