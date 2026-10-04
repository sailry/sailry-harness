//! Durable orchestration records share the Node database and command admission.
use super::{
    commands::check_revision,
    database::{encode, storage_error},
};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{self as protocol, ErrorCode, Fault, dispatch::*, plugin::Reference};
use serde::{Serialize, de::DeserializeOwned};

mod events;
mod handlers;
pub(crate) mod jobs;
mod schedules;
mod worker;
pub(crate) use worker::Batch;
pub(super) use worker::{finish, tick};
#[cfg(test)]
mod tests;

pub(super) fn execute(
    db: &Connection,
    package: &Reference,
    action: &Command,
) -> Result<(protocol::Output, Option<protocol::Event>), Fault> {
    available(db, package)?;
    execute_admitted(db, package, action)
}

/// The caller has checked the original running invocation and its declared scope.
pub(super) fn execute_admitted(
    db: &Connection,
    package: &Reference,
    action: &Command,
) -> Result<(protocol::Output, Option<protocol::Event>), Fault> {
    let mut packages = vec![package.name.clone()];
    let (output, changed) = match action {
        Command::ListHandlers => (
            Output::Handlers(list(db, "dispatch_handlers", &package.name)?),
            false,
        ),
        Command::SaveHandler(handler) => {
            (Output::Handler(handlers::save(db, package, handler)?), true)
        }
        Command::RemoveHandler {
            name,
            expected_revision,
            cancel_pending,
        } => {
            let old: Handler = required(db, "dispatch_handlers", &package.name, "name", name)?;
            check_revision(old.revision, *expected_revision)?;
            if *cancel_pending {
                jobs::cancel_pending(db, &package.name, name)?;
            }
            db.execute(
                "DELETE FROM dispatch_handlers WHERE package=?1 AND name=?2",
                params![package.name, name],
            )
            .map_err(storage_error)?;
            (Output::Removed, true)
        }
        Command::ListSchedules => (
            Output::Schedules(list(db, "dispatch_schedules", &package.name)?),
            false,
        ),
        Command::SaveSchedule(schedule) => (
            Output::Schedule(schedules::save(db, &package.name, schedule)?),
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
            )?;
            check_revision(old.revision, *expected_revision)?;
            db.execute(
                "DELETE FROM dispatch_schedules WHERE package=?1 AND id=?2",
                params![package.name, id.to_string()],
            )
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
            let (jobs, owners) = events::publish(db, &event)?;
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
                required(db, "dispatch_handlers", &package.name, "name", handler)?;
            if !handler.enabled {
                return Err(invalid("event handler is disabled"));
            }
            let reference = current(db, &package.name)?.summary.reference();
            available(db, &reference)?;
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
                Output::Job(events::enqueue(db, &reference, &handler, &event)?),
                true,
            )
        }
        Command::ListJobs { before, limit } => (
            Output::Jobs(jobs::list(db, &package.name, *before, *limit)?),
            false,
        ),
        Command::ReadJob { id } => (
            Output::Job(required(
                db,
                "dispatch_jobs",
                &package.name,
                "id",
                &id.to_string(),
            )?),
            false,
        ),
        Command::CancelJob { id } => (Output::Job(jobs::cancel(db, &package.name, *id)?), true),
        Command::ReadResult { id } => {
            (Output::Result(jobs::result(db, &package.name, *id)?), false)
        }
    };
    Ok((
        protocol::Output::Dispatch(output),
        changed.then_some(protocol::Event::DispatchChanged { packages }),
    ))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

fn name(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err(invalid(
            "dispatch names must use letters, digits, dots, underscores or hyphens",
        ));
    }
    Ok(())
}

fn data(value: &impl Serialize) -> Result<(), Fault> {
    if encode(value)?.len() > MAX_PAYLOAD_BYTES {
        return Err(invalid("dispatch payload exceeds the size limit"));
    }
    Ok(())
}

pub(super) fn available(db: &Connection, package: &Reference) -> Result<(), Fault> {
    let installed = current(db, &package.name)?;
    if installed.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "dispatch plugin changed",
        ));
    }
    Ok(())
}

fn current(db: &Connection, name: &str) -> Result<protocol::plugin::Info, Fault> {
    let installed = super::plugins::get(db, name)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "dispatch plugin is not installed"))?;
    if !installed.summary.enabled {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "dispatch plugin is disabled",
        ));
    }
    Ok(installed)
}

fn capacity(db: &Connection, table: &str, package: &str) -> Result<(), Fault> {
    let count: i64 = db
        .query_row(
            &format!("SELECT count(*) FROM {table} WHERE package=?1"),
            [package],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if count >= MAX_ITEMS as i64 {
        return Err(Fault::new(
            ErrorCode::Busy,
            "dispatch configuration capacity exhausted",
        ));
    }
    Ok(())
}

fn budget(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
    value: &impl Serialize,
) -> Result<(), Fault> {
    let total: i64 = db
        .query_row(
            &format!(
                "SELECT coalesce(sum(length(body)),0) FROM {table} WHERE package=?1 AND {key}!=?2"
            ),
            params![package, id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if total as usize + encode(value)?.len() > 4 * 1024 * 1024 {
        return Err(Fault::new(
            ErrorCode::Busy,
            "dispatch namespace capacity exhausted",
        ));
    }
    Ok(())
}

fn get<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
) -> Result<Option<T>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            &format!("SELECT body FROM {table} WHERE package=?1 AND {key}=?2"),
            params![package, id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

fn required<T: DeserializeOwned>(
    db: &Connection,
    table: &str,
    package: &str,
    key: &str,
    id: &str,
) -> Result<T, Fault> {
    get(db, table, package, key, id)?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "dispatch resource is unavailable"))
}

fn list<T: DeserializeOwned>(db: &Connection, table: &str, package: &str) -> Result<Vec<T>, Fault> {
    let mut query = db
        .prepare(&format!(
            "SELECT body FROM {table} WHERE package=?1 ORDER BY rowid"
        ))
        .map_err(storage_error)?;
    query
        .query_map([package], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
        .collect()
}
