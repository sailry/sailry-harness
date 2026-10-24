use super::*;

pub(super) async fn save(
    db: &Connection,
    package: &str,
    schedule: &Schedule,
) -> Result<Schedule, Fault> {
    name(&schedule.topic)?;
    data(&schedule.payload)?;
    let mut saved = schedule.clone();
    if let Timing::Weekly { weekdays, .. } = &mut saved.timing {
        weekdays.sort_unstable();
    }
    let first = match &saved.timing {
        Timing::Once { at_ms } => *at_ms,
        Timing::Every {
            anchor_ms,
            interval_ms,
        } => {
            let interval = interval(*interval_ms)?;
            anchor_ms
                .checked_add(interval)
                .ok_or_else(|| Fault::invalid("schedule time is not representable"))?;
            *anchor_ms
        }
        Timing::Weekly {
            timezone,
            weekdays,
            hour,
            minute,
        } => calendar::Calendar::new(timezone, weekdays, *hour, *minute)?
            .next(now())?
            .ok_or_else(|| Fault::invalid("schedule has no future occurrence"))?,
    };
    if first < 0 {
        return Err(sailry_protocol::Fault::invalid("schedule time is invalid"));
    }
    let old: Option<Schedule> = get(
        db,
        "dispatch_schedules",
        package,
        "id",
        &schedule.id.to_string(),
    )
    .await?;
    check_revision(
        old.as_ref().map_or(0, |old| old.revision),
        schedule.revision,
    )?;
    if old.is_none() {
        capacity(db, "dispatch_schedules", package).await?;
    }
    saved.revision += 1;
    // Pausing suppresses the due index, not the consumed cursor. Re-enabling a
    // completed one-shot must not repeat an already delivered effect.
    saved.next_ms = match old.filter(|old| old.timing == saved.timing) {
        Some(old) => old.next_ms,
        None => Some(first),
    };
    put(db, package, &saved).await?;
    Ok(saved)
}

async fn put(db: &Connection, package: &str, schedule: &Schedule) -> Result<(), Fault> {
    budget(
        db,
        "dispatch_schedules",
        package,
        "id",
        &schedule.id.to_string(),
        schedule,
    )
    .await?;
    db.execute("INSERT INTO dispatch_schedules(package,id,next_ms,body) VALUES(?1,?2,?3,?4) ON CONFLICT(package,id) DO UPDATE SET next_ms=excluded.next_ms,body=excluded.body", params![package, schedule.id.to_string(), schedule.enabled.then_some(schedule.next_ms).flatten(), encode(schedule)?]).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn due(db: &Connection, timestamp: i64) -> Result<Vec<String>, Fault> {
    let enabled: std::collections::BTreeSet<_> = super::super::plugins::list(db)
        .await?
        .into_iter()
        .filter(|info| info.enabled)
        .map(|info| info.name)
        .collect();
    let mut query = db
        .prepare("SELECT package,id FROM dispatch_schedules WHERE next_ms<=?1 ORDER BY next_ms")
        .await
        .map_err(storage_error)?;
    let records = {
        let mut rows = query.query([timestamp]).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .filter(|row| {
        row.as_ref().map_or(true, |(package, _)| {
            package == native::OWNER || enabled.contains(package)
        })
    })
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    let mut changed = Vec::new();
    let mut attempted = 0;
    for (package, id) in records {
        if attempted == 64 {
            break;
        }
        let mut schedule: Schedule =
            required(db, "dispatch_schedules", &package, "id", &id).await?;
        let (scheduled_ms, next_ms) = match occurrence(&schedule, timestamp) {
            Ok(occurrence) => occurrence,
            Err(error) if error.code == ErrorCode::InvalidRequest => {
                // Keep the invalid cursor available for repair without starving valid records.
                eprintln!(
                    "dispatch schedule is invalid: package={package} id={id} code={:?}",
                    error.code
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        attempted += 1;
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
            .await
            .map_err(storage_error)?;
        match events::publish(db, &event).await {
            Ok((_, owners)) => changed.extend(owners),
            Err(error) => {
                db.execute_batch("ROLLBACK TO dispatch_due; RELEASE dispatch_due")
                    .await
                    .map_err(storage_error)?;
                if error.code == ErrorCode::Busy {
                    continue;
                }
                return Err(error);
            }
        }
        db.execute_batch("RELEASE dispatch_due")
            .await
            .map_err(storage_error)?;
        schedule.next_ms = next_ms;
        put(db, &package, &schedule).await?;
    }
    changed.sort();
    changed.dedup();
    Ok(changed)
}

pub(super) async fn next(db: &Connection, timestamp: i64) -> Result<Option<i64>, Fault> {
    let mut query = db.prepare("SELECT package,min(next_ms) FROM dispatch_schedules WHERE next_ms IS NOT NULL GROUP BY package").await.map_err(storage_error)?;
    let rows = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<i64>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    };
    let mut next: Option<i64> = None;
    for row in rows {
        let (package, due) = row.map_err(storage_error)?;
        if package == native::OWNER
            || super::super::plugins::get(db, &package)
                .await?
                .is_some_and(|info| info.summary.enabled)
        {
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

fn occurrence(schedule: &Schedule, timestamp: i64) -> Result<(i64, Option<i64>), Fault> {
    let first = schedule
        .next_ms
        .filter(|first| *first >= 0 && *first <= timestamp)
        .ok_or_else(|| Fault::invalid("due schedule has an invalid cursor"))?;
    match &schedule.timing {
        Timing::Once { .. } => Ok((first, None)),
        Timing::Every { interval_ms, .. } => {
            let interval = interval(*interval_ms)?;
            let latest = first + ((timestamp - first) / interval) * interval;
            Ok((latest, latest.checked_add(interval)))
        }
        Timing::Weekly {
            timezone,
            weekdays,
            hour,
            minute,
        } => calendar::Calendar::new(timezone, weekdays, *hour, *minute)?.due(first, timestamp),
    }
}

fn interval(milliseconds: u64) -> Result<i64, Fault> {
    i64::try_from(milliseconds)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| Fault::invalid("schedule interval must be positive and representable"))
}
