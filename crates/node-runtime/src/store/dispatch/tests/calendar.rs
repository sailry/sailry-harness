use super::*;

fn at(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

async fn schedule(fixture: &Fixture) -> Schedule {
    fixture.setup("weekday").await;
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
    .await
    .unwrap()
}

async fn cursor(fixture: &Fixture, schedule: &mut Schedule, timestamp: i64) {
    schedule.next_ms = Some(timestamp);
    fixture
        .database
        .connection
        .execute(
            "UPDATE dispatch_schedules SET next_ms=?1,body=?2 WHERE package=?3 AND id=?4",
            params![
                timestamp,
                encode(schedule).unwrap(),
                fixture.package.name.as_str(),
                schedule.id.to_string()
            ],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn metadata_edits_preserve_consumed_cursor() {
    let fixture = Fixture::new().await;
    let mut item = schedule(&fixture).await;
    let original = item.next_ms;
    assert!(original.unwrap() > now());
    assert!(
        matches!(&item.timing, Timing::Weekly { weekdays, .. } if weekdays == &[1, 2, 3, 4, 5])
    );
    for enabled in [false, true] {
        item.enabled = enabled;
        item.payload = json!("renamed");
        item = schedules::save(&fixture.database.connection, &fixture.package.name, &item)
            .await
            .unwrap();
        assert_eq!(item.next_ms, original);
    }
    assert!(
        jobs::list(
            &fixture.database.connection,
            &fixture.package.name,
            None,
            100
        )
        .await
        .unwrap()
        .jobs
        .is_empty()
    );
}

#[tokio::test]
async fn downtime_consumes_latest_occurrence_once() {
    let fixture = Fixture::new().await;
    let mut item = schedule(&fixture).await;
    cursor(&fixture, &mut item, at("2020-01-06T09:00:00Z")).await;
    let db = &fixture.database.connection;
    let observed = at("2026-10-21T10:00:00Z");
    assert_eq!(
        schedules::due(db, observed).await.unwrap(),
        vec![fixture.package.name.clone()]
    );
    assert!(schedules::due(db, observed).await.unwrap().is_empty());
    let page = jobs::list(db, &fixture.package.name, None, 100)
        .await
        .unwrap();
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
    .await
    .unwrap();
    assert_eq!(stored.next_ms, Some(at("2026-10-22T09:00:00Z")));
    assert_eq!(stored.revision, item.revision);
}

#[tokio::test]
async fn full_queue_preserves_due_cursor() {
    let fixture = Fixture::new().await;
    let mut item = schedule(&fixture).await;
    let first = at("2026-10-19T09:00:00Z");
    cursor(&fixture, &mut item, first).await;
    let db = &fixture.database.connection;
    let handler: Handler = required(
        db,
        "dispatch_handlers",
        &fixture.package.name,
        "name",
        "weekday",
    )
    .await
    .unwrap();
    db.execute_batch("BEGIN CONCURRENT").await.unwrap();
    let mut first_job = None;
    for _ in 0..MAX_PENDING {
        let job = events::enqueue(
            db,
            Some(&fixture.package),
            &handler,
            &fixture.event("weekday"),
        )
        .await
        .unwrap();
        first_job.get_or_insert(job.id);
    }
    db.execute_batch("COMMIT").await.unwrap();
    assert!(schedules::due(db, first).await.unwrap().is_empty());
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &item.id.to_string(),
    )
    .await
    .unwrap();
    assert_eq!(stored, item);
    let events: i64 = (async {
        let mut statement = db.prepare("SELECT count(*) FROM dispatch_events").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(events, 0);
    jobs::cancel(db, &fixture.package.name, first_job.unwrap())
        .await
        .unwrap();
    assert_eq!(
        schedules::due(db, first + 1).await.unwrap(),
        vec![fixture.package.name.clone()]
    );
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &item.id.to_string(),
    )
    .await
    .unwrap();
    assert_eq!(stored.next_ms, Some(at("2026-10-20T09:00:00Z")));
}
