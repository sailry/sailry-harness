//! The backup settings owner registers one controller-only scheduled command.
use super::*;
use std::collections::BTreeMap;

pub(super) const OWNER: &str = "";
const HANDLER: &str = "profile-backup";
const SCHEDULE: &str = "40ca706e-25c1-45ca-bacf-61ee028fb608";

pub(in crate::store) fn configure(
    db: &Connection,
    settings: &protocol::backup::Settings,
) -> Result<(), Fault> {
    let id: protocol::ScheduleId = SCHEDULE.parse().expect("fixed schedule identifier");
    let previous: Option<Schedule> = get(db, "dispatch_schedules", OWNER, "id", SCHEDULE)?;
    if settings.schedule.is_none() && previous.is_none() {
        return Ok(());
    }
    let timing = match &settings.schedule {
        Some(schedule) => Timing::Weekly {
            timezone: schedule.timezone.clone(),
            weekdays: vec![1, 2, 3, 4, 5, 6, 7],
            hour: schedule.hour,
            minute: schedule.minute,
        },
        None => previous
            .as_ref()
            .expect("existing disabled schedule")
            .timing
            .clone(),
    };
    let enabled = settings
        .schedule
        .as_ref()
        .is_some_and(|schedule| schedule.enabled);
    let previous_handler: Option<Handler> = get(db, "dispatch_handlers", OWNER, "name", HANDLER)?;
    let handler = Handler {
        name: HANDLER.into(),
        revision: previous_handler
            .map_or(0, |handler| handler.revision)
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| Fault::invalid("backup handler revision is exhausted"))?,
        enabled,
        source: Source {
            package: OWNER.into(),
            topic: HANDLER.into(),
        },
        queue: HANDLER.into(),
        callback: Callback {
            scope: Scope::default(),
            completion: Completion::Command,
            command: Box::new(protocol::Command::ExportBackup {
                expected_revision: settings.revision,
            }),
            bindings: BTreeMap::new(),
        },
    };
    db.execute("INSERT INTO dispatch_handlers(package,name,source,topic,body,reference) VALUES(?1,?2,?1,?2,?3,?4) ON CONFLICT(package,name) DO UPDATE SET body=excluded.body,reference=excluded.reference", params![OWNER,HANDLER,encode(&handler)?,b"null".as_slice()]).map_err(storage_error)?;
    jobs::cancel_pending(db, OWNER, HANDLER)?;
    schedules::save(
        db,
        OWNER,
        &Schedule {
            id,
            revision: previous.as_ref().map_or(0, |schedule| schedule.revision),
            enabled,
            topic: HANDLER.into(),
            payload: serde_json::Value::Null,
            timing,
            next_ms: None,
        },
    )?;
    Ok(())
}

pub(super) fn handler(handler: &Handler) -> Result<(), Fault> {
    if handler.name != HANDLER
        || handler.source.package != OWNER
        || handler.source.topic != HANDLER
        || handler.queue != HANDLER
        || handler.callback.scope != Scope::default()
        || handler.callback.completion != Completion::Command
        || !handler.callback.bindings.is_empty()
        || !matches!(
            *handler.callback.command,
            protocol::Command::ExportBackup { .. }
        )
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "native callback is not a registered backup",
        ));
    }
    Ok(())
}

pub(super) fn callback(owner: &str, job: &Job, callback: &Callback) -> Result<(), Fault> {
    if owner != OWNER || job.handler != HANDLER || job.queue != HANDLER {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "callback has no native owner",
        ));
    }
    handler(&Handler {
        name: job.handler.clone(),
        revision: 0,
        enabled: true,
        source: job.event.source.clone(),
        queue: job.queue.clone(),
        callback: callback.clone(),
    })
}

pub(in crate::store) fn validate_time(schedule: &protocol::backup::Schedule) -> Result<(), Fault> {
    calendar::Calendar::new(
        &schedule.timezone,
        &[1, 2, 3, 4, 5, 6, 7],
        schedule.hour,
        schedule.minute,
    )
    .map(|_| ())
}
