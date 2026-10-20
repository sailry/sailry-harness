use super::*;

fn at(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

fn schedule(fixture: &Fixture) -> Schedule {
    fixture.setup("weekday");
    schedules::save(
        &fixture.database.connection,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "weekday".into(),
            payload: json!(null),
            timing: Timing::Weekly {
                timezone: "UTC".into(),
                weekdays: vec![5, 1, 2, 3, 4],
                hour: 9,
                minute: 0,
            },
            next_ms: Some(1),
        },
    )
    .unwrap()
}

fn cursor(fixture: &Fixture, schedule: &mut Schedule, timestamp: i64) {
    schedule.next_ms = Some(timestamp);
    fixture
        .database
        .connection
        .execute(
            "UPDATE dispatch_schedules SET next_ms=?1,body=?2 WHERE package=?3 AND id=?4",
            params![
                timestamp,
                encode(schedule).unwrap(),
                fixture.package.name,
                schedule.id.to_string()
            ],
        )
        .unwrap();
}

#[test]
fn metadata_edits_preserve_consumed_cursor() {
    let fixture = Fixture::new();
    let mut item = schedule(&fixture);
    let original = item.next_ms;
    assert!(original.unwrap() > now());
    assert!(
        matches!(&item.timing, Timing::Weekly { weekdays, .. } if weekdays == &[1, 2, 3, 4, 5])
    );
    for enabled in [false, true] {
        item.enabled = enabled;
        item.payload = json!("renamed");
        item = schedules::save(&fixture.database.connection, &fixture.package.name, &item).unwrap();
        assert_eq!(item.next_ms, original);
    }
    assert!(
        jobs::list(
            &fixture.database.connection,
            &fixture.package.name,
            None,
            100
        )
        .unwrap()
        .jobs
        .is_empty()
    );
}

#[test]
fn downtime_consumes_latest_occurrence_once() {
    let fixture = Fixture::new();
    let mut item = schedule(&fixture);
    cursor(&fixture, &mut item, at("2020-01-06T09:00:00Z"));
    let db = &fixture.database.connection;
    let observed = at("2026-10-21T10:00:00Z");
    assert_eq!(
        schedules::due(db, observed).unwrap(),
        vec![fixture.package.name.clone()]
    );
    assert!(schedules::due(db, observed).unwrap().is_empty());
    let page = jobs::list(db, &fixture.package.name, None, 100).unwrap();
    assert_eq!(page.jobs.len(), 1);
    assert_eq!(
        page.jobs[0].event.scheduled_ms,
        Some(at("2026-10-21T09:00:00Z"))
    );
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &item.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, Some(at("2026-10-22T09:00:00Z")));
    assert_eq!(stored.revision, item.revision);
}

#[test]
fn full_queue_preserves_due_cursor() {
    let fixture = Fixture::new();
    let mut item = schedule(&fixture);
    let first = at("2026-10-19T09:00:00Z");
    cursor(&fixture, &mut item, first);
    let db = &fixture.database.connection;
    let handler: Handler = required(
        db,
        "dispatch_handlers",
        &fixture.package.name,
        "name",
        "weekday",
    )
    .unwrap();
    let mut first_job = None;
    for _ in 0..MAX_PENDING {
        let job = events::enqueue(
            db,
            Some(&fixture.package),
            &handler,
            &fixture.event("weekday"),
        )
        .unwrap();
        first_job.get_or_insert(job.id);
    }
    assert!(schedules::due(db, first).unwrap().is_empty());
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &item.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored, item);
    let events: i64 = db
        .query_row("SELECT count(*) FROM dispatch_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 0);
    jobs::cancel(db, &fixture.package.name, first_job.unwrap()).unwrap();
    assert_eq!(
        schedules::due(db, first + 1).unwrap(),
        vec![fixture.package.name.clone()]
    );
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &item.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, Some(at("2026-10-20T09:00:00Z")));
}
