use super::*;

pub(super) fn save(db: &Connection, package: &str, schedule: &Schedule) -> Result<Schedule, Fault> {
    name(&schedule.topic)?;
    data(&schedule.payload)?;
    let first = match schedule.timing {
        Timing::Once { at_ms } => at_ms,
        Timing::Every {
            anchor_ms,
            interval_ms,
        } => {
            if !(1000..=31_536_000_000).contains(&interval_ms) {
                return Err(invalid(
                    "schedule interval must be between one second and one year",
                ));
            }
            anchor_ms
        }
    };
    if first < 0 {
        return Err(invalid("schedule time is invalid"));
    }
    let old: Option<Schedule> = get(
        db,
        "dispatch_schedules",
        package,
        "id",
        &schedule.id.to_string(),
    )?;
    check_revision(
        old.as_ref().map_or(0, |old| old.revision),
        schedule.revision,
    )?;
    if old.is_none() {
        capacity(db, "dispatch_schedules", package)?;
    }
    let mut saved = schedule.clone();
    saved.revision += 1;
    // Pausing suppresses the due index, not the consumed cursor. Re-enabling a
    // completed one-shot must not repeat an already delivered effect.
    saved.next_ms = match old.filter(|old| old.timing == saved.timing) {
        Some(old) => old.next_ms,
        None => Some(first),
    };
    put(db, package, &saved)?;
    Ok(saved)
}

fn put(db: &Connection, package: &str, schedule: &Schedule) -> Result<(), Fault> {
    budget(
        db,
        "dispatch_schedules",
        package,
        "id",
        &schedule.id.to_string(),
        schedule,
    )?;
    db.execute("INSERT INTO dispatch_schedules(package,id,next_ms,body) VALUES(?1,?2,?3,?4) ON CONFLICT(package,id) DO UPDATE SET next_ms=excluded.next_ms,body=excluded.body", params![package, schedule.id.to_string(), schedule.enabled.then_some(schedule.next_ms).flatten(), encode(schedule)?]).map_err(storage_error)?;
    Ok(())
}

pub(super) fn due(db: &Connection, timestamp: i64) -> Result<Vec<String>, Fault> {
    let enabled: std::collections::BTreeSet<_> = super::super::plugins::list(db)?
        .into_iter()
        .filter(|info| info.enabled)
        .map(|info| info.name)
        .collect();
    let mut query = db
        .prepare("SELECT package,id FROM dispatch_schedules WHERE next_ms<=?1 ORDER BY next_ms")
        .map_err(storage_error)?;
    let records = query
        .query_map([timestamp], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .filter(|row| {
            row.as_ref()
                .map_or(true, |(package, _)| enabled.contains(package))
        })
        .take(64)
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let mut changed = Vec::new();
    for (package, id) in records {
        let mut schedule: Schedule = required(db, "dispatch_schedules", &package, "id", &id)?;
        let first = schedule
            .next_ms
            .ok_or_else(|| invalid("due schedule has no cursor"))?;
        let (scheduled_ms, next_ms) = match schedule.timing {
            Timing::Once { .. } => (first, None),
            Timing::Every { interval_ms, .. } => {
                let interval = interval_ms as i64;
                let latest = first + ((timestamp - first) / interval) * interval;
                (latest, latest.checked_add(interval))
            }
        };
        let event = Event {
            id: protocol::EventId::new(),
            source: Source {
                package: package.clone(),
                topic: schedule.topic.clone(),
            },
            payload: schedule.payload.clone(),
            timestamp_ms: timestamp,
            schedule: Some(schedule.id),
            scheduled_ms: Some(scheduled_ms),
        };
        db.execute_batch("SAVEPOINT dispatch_due")
            .map_err(storage_error)?;
        match events::publish(db, &event) {
            Ok((_, owners)) => changed.extend(owners),
            Err(error) => {
                db.execute_batch("ROLLBACK TO dispatch_due; RELEASE dispatch_due")
                    .map_err(storage_error)?;
                if error.code == ErrorCode::Busy {
                    continue;
                }
                return Err(error);
            }
        }
        db.execute_batch("RELEASE dispatch_due")
            .map_err(storage_error)?;
        schedule.next_ms = next_ms;
        put(db, &package, &schedule)?;
    }
    changed.sort();
    changed.dedup();
    Ok(changed)
}

pub(super) fn next(db: &Connection, timestamp: i64) -> Result<Option<i64>, Fault> {
    let mut query = db.prepare("SELECT package,min(next_ms) FROM dispatch_schedules WHERE next_ms IS NOT NULL GROUP BY package").map_err(storage_error)?;
    let rows = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(storage_error)?;
    let mut next: Option<i64> = None;
    for row in rows {
        let (package, due) = row.map_err(storage_error)?;
        if super::super::plugins::get(db, &package)?.is_some_and(|info| info.summary.enabled) {
            // A full delivery queue leaves its schedule unconsumed. Completions
            // wake us sooner; a bounded retry avoids spinning on overdue ticks.
            let due = if due <= timestamp {
                timestamp.saturating_add(1000)
            } else {
                due
            };
            next = Some(next.map_or(due, |previous| previous.min(due)));
        }
    }
    Ok(next)
}
