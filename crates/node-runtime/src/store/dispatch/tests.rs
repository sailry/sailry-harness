use super::*;
use crate::store::Database;
use serde_json::json;
use std::collections::BTreeMap;

#[path = "tests/calendar.rs"]
mod calendar;

struct Fixture {
    database: Database,
    package: Reference,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(
            &directory
                .path()
                .canonicalize()
                .unwrap()
                .join("node.sqlite3"),
            protocol::NodeId([61; 32]),
            None,
        )
        .unwrap();
        let package = install(&database.connection, directory.path(), "dispatch-source")
            .summary
            .reference();
        Self {
            database,
            package,
            _directory: directory,
        }
    }
    fn setup(&self, group: &str) -> Handler {
        let db = &self.database.connection;
        handlers::save(
            db,
            &self.package,
            &Handler {
                name: group.into(),
                revision: 0,
                enabled: true,
                source: Source {
                    package: self.package.name.clone(),
                    topic: group.into(),
                },
                queue: group.into(),
                callback: Callback {
                    scope: Scope::default(),
                    completion: Completion::Command,
                    command: Box::new(protocol::Command::ReadPluginValue {
                        key: "fixture".into(),
                    }),
                    bindings: BTreeMap::new(),
                },
            },
        )
        .unwrap()
    }
    fn event(&self, topic: &str) -> Event {
        Event {
            id: protocol::EventId::new(),
            source: Source {
                package: self.package.name.clone(),
                topic: topic.into(),
            },
            payload: json!({"key":"bound"}),
            timestamp_ms: 100,
            schedule: None,
            scheduled_ms: None,
        }
    }
}

#[test]
fn schedule_pause_preserves_consumed_cursor() {
    let fixture = Fixture::new();
    fixture.setup("once");
    let db = &fixture.database.connection;
    let mut schedule = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: false,
            topic: "once".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1000 },
            next_ms: None,
        },
    )
    .unwrap();
    assert!(schedules::due(db, 2000).unwrap().is_empty());
    schedule.enabled = true;
    schedule = schedules::save(db, &fixture.package.name, &schedule).unwrap();
    assert!(!schedules::due(db, 2000).unwrap().is_empty());
    schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &schedule.id.to_string(),
    )
    .unwrap();
    assert_eq!(schedule.next_ms, None);
    schedule.enabled = false;
    schedule = schedules::save(db, &fixture.package.name, &schedule).unwrap();
    schedule.enabled = true;
    schedules::save(db, &fixture.package.name, &schedule).unwrap();
    assert!(schedules::due(db, 3000).unwrap().is_empty());
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .len(),
        1
    );
}

#[test]
fn claims_same_group_beyond_previous_limits() {
    let fixture = Fixture::new();
    let first = fixture.setup("first");
    let second = fixture.setup("second");
    let db = &fixture.database.connection;
    let mut first_ids = Vec::new();
    for _ in 0..24 {
        first_ids.push(
            events::enqueue(db, Some(&fixture.package), &first, &fixture.event("first"))
                .unwrap()
                .id,
        );
    }
    for _ in 0..20 {
        events::enqueue(
            db,
            Some(&fixture.package),
            &second,
            &fixture.event("second"),
        )
        .unwrap();
    }
    let (work, _) = jobs::claim(db, fixture.database.node, 200).unwrap();
    assert_eq!(work.len(), 44);
    assert_eq!(work[0].job, first_ids[0]);
    assert!(
        jobs::claim(db, fixture.database.node, 201)
            .unwrap()
            .0
            .is_empty()
    );
    let next =
        events::enqueue(db, Some(&fixture.package), &first, &fixture.event("first")).unwrap();
    let (work, _) = jobs::claim(db, fixture.database.node, 202).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].job, next.id);
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .iter()
            .filter(|job| job.status == Status::Running)
            .count(),
        45
    );
}

#[test]
fn claims_drain_during_callbacks() {
    let mut fixture = Fixture::new();
    let handler = fixture.setup("batch");
    let db = &fixture.database.connection;
    for _ in 0..MAX_PENDING {
        events::enqueue(
            db,
            Some(&fixture.package),
            &handler,
            &fixture.event("batch"),
        )
        .unwrap();
    }
    let (events, _) = tokio::sync::broadcast::channel(16);
    for batch in 0..8 {
        let next = tick(&mut fixture.database, &events, false).unwrap();
        assert_eq!(next.work.len(), 128);
        assert_eq!(next.pending, batch < 7);
    }
    let last = events::enqueue(
        &fixture.database.connection,
        Some(&fixture.package),
        &handler,
        &fixture.event("batch"),
    )
    .unwrap();
    let next = tick(&mut fixture.database, &events, false).unwrap();
    assert_eq!(next.work.len(), 1);
    assert_eq!(next.work[0].job, last.id);
    assert!(!next.pending);
    let running: i64 = fixture
        .database
        .connection
        .query_row(
            "SELECT count(*) FROM dispatch_jobs WHERE status='running'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(running, 1025);
}

#[test]
fn cancels_only_unstarted_jobs() {
    let fixture = Fixture::new();
    let handler = fixture.setup("queue");
    let db = &fixture.database.connection;
    let queued = events::enqueue(
        db,
        Some(&fixture.package),
        &handler,
        &fixture.event("queue"),
    )
    .unwrap();
    assert_eq!(
        jobs::cancel(db, &fixture.package.name, queued.id)
            .unwrap()
            .status,
        Status::Cancelled
    );
    assert_eq!(
        jobs::cancel(db, &fixture.package.name, queued.id)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let running = events::enqueue(
        db,
        Some(&fixture.package),
        &handler,
        &fixture.event("queue"),
    )
    .unwrap();
    assert_eq!(
        jobs::claim(db, fixture.database.node, 200).unwrap().0.len(),
        1
    );
    assert_eq!(
        jobs::cancel(db, &fixture.package.name, running.id)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn schedules_coalesce_and_consume_once() {
    let fixture = Fixture::new();
    fixture.setup("tick");
    let db = &fixture.database.connection;
    let schedule = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "tick".into(),
            payload: json!("scheduled"),
            timing: Timing::Every {
                anchor_ms: 1000,
                interval_ms: 1000,
            },
            next_ms: Some(99),
        },
    )
    .unwrap();
    assert_eq!(schedule.next_ms, Some(1000));
    schedules::due(db, 5500).unwrap();
    let page = jobs::list(db, &fixture.package.name, None, 100).unwrap();
    assert_eq!(page.jobs.len(), 1);
    assert_eq!(page.jobs[0].event.scheduled_ms, Some(5000));
    assert_eq!(page.jobs[0].event.schedule, Some(schedule.id));
    schedules::due(db, 5500).unwrap();
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .len(),
        1
    );
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &schedule.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, Some(6000));
    let once = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "tick".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1500 },
            next_ms: None,
        },
    )
    .unwrap();
    schedules::due(db, 5500).unwrap();
    schedules::due(db, 5500).unwrap();
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &once.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, None);
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .len(),
        2
    );
}

#[test]
fn schedules_preserve_representable_intervals() {
    for interval_ms in [1, 31_536_000_001, 9_007_199_254_740_991] {
        let fixture = Fixture::new();
        fixture.setup("tick");
        let db = &fixture.database.connection;
        let schedule = schedules::save(
            db,
            &fixture.package.name,
            &Schedule {
                id: protocol::ScheduleId::new(),
                revision: 0,
                enabled: true,
                topic: "tick".into(),
                payload: json!(null),
                timing: Timing::Every {
                    anchor_ms: 1000,
                    interval_ms,
                },
                next_ms: None,
            },
        )
        .unwrap();
        let tick = 1000 + i64::try_from(interval_ms).unwrap();
        schedules::due(db, tick).unwrap();
        let page = jobs::list(db, &fixture.package.name, None, 100).unwrap();
        assert_eq!(page.jobs.len(), 1);
        assert_eq!(page.jobs[0].event.scheduled_ms, Some(tick));
        let stored: Schedule = required(
            db,
            "dispatch_schedules",
            &fixture.package.name,
            "id",
            &schedule.id.to_string(),
        )
        .unwrap();
        assert_eq!(stored.next_ms, tick.checked_add(interval_ms as i64));
    }
}

#[test]
fn schedules_reject_unrepresentable_times() {
    let fixture = Fixture::new();
    let db = &fixture.database.connection;
    for (anchor_ms, interval_ms) in [(0, 0), (0, u64::MAX), (i64::MAX, 1)] {
        let schedule = Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "tick".into(),
            payload: json!(null),
            timing: Timing::Every {
                anchor_ms,
                interval_ms,
            },
            next_ms: None,
        };
        assert_eq!(
            schedules::save(db, &fixture.package.name, &schedule)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let stored: Option<Schedule> = get(
            db,
            "dispatch_schedules",
            &fixture.package.name,
            "id",
            &schedule.id.to_string(),
        )
        .unwrap();
        assert!(stored.is_none());
    }
}

#[test]
fn callbacks_preserve_event_data_and_request_identity() {
    let fixture = Fixture::new();
    let mut handler = fixture.setup("bound");
    handler
        .callback
        .bindings
        .insert("/data/key".into(), "/payload/key".into());
    let db = &fixture.database.connection;
    let handler = handlers::save(db, &fixture.package, &handler).unwrap();
    let job = events::enqueue(
        db,
        Some(&fixture.package),
        &handler,
        &fixture.event("bound"),
    )
    .unwrap();
    let (mut work, _) = jobs::claim(db, fixture.database.node, 200).unwrap();
    let request = work.remove(0).request;
    assert_eq!(request.id, job.request);
    assert_eq!(
        request.command,
        protocol::Command::ReadPluginValue {
            key: "bound".into()
        }
    );
    assert_eq!(request.plugin.unwrap().package, fixture.package);
}

#[test]
fn recovery_does_not_replay_interrupted_effects() {
    let fixture = Fixture::new();
    let handler = fixture.setup("recover");
    let db = &fixture.database.connection;
    let job = events::enqueue(
        db,
        Some(&fixture.package),
        &handler,
        &fixture.event("recover"),
    )
    .unwrap();
    jobs::claim(db, fixture.database.node, 200).unwrap();
    jobs::recover(db, fixture.database.node).unwrap();
    let recovered: Job = required(
        db,
        "dispatch_jobs",
        &fixture.package.name,
        "id",
        &job.id.to_string(),
    )
    .unwrap();
    assert_eq!(recovered.status, Status::Unknown);
    assert_eq!(recovered.error.unwrap().code, ErrorCode::OutcomeUnknown);
    assert!(
        jobs::claim(db, fixture.database.node, 201)
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn bounds_and_revisions_are_enforced() {
    let fixture = Fixture::new();
    let handler = fixture.setup("bounds");
    let db = &fixture.database.connection;
    let mut stale = handler.clone();
    stale.revision = 0;
    assert_eq!(
        handlers::save(db, &fixture.package, &stale)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut invalid = handler;
    invalid
        .callback
        .bindings
        .insert("/kind".into(), "/payload/key".into());
    assert_eq!(
        handlers::save(db, &fixture.package, &invalid)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 0)
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn recovery_uses_committed_receipts() {
    let fixture = Fixture::new();
    let handler = fixture.setup("receipt");
    let db = &fixture.database.connection;
    let job = events::enqueue(
        db,
        Some(&fixture.package),
        &handler,
        &fixture.event("receipt"),
    )
    .unwrap();
    let (work, _) = jobs::claim(db, fixture.database.node, 200).unwrap();
    let result: Result<protocol::Output, Fault> = Ok(protocol::Output::Dispatch(Output::Removed));
    db.execute(
        "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
        params![
            &fixture.database.node.0[..],
            job.request.to_string(),
            encode(&work[0].request).unwrap(),
            serde_json::to_string(&result).unwrap()
        ],
    )
    .unwrap();
    jobs::recover(db, fixture.database.node).unwrap();
    let recovered: Job = required(
        db,
        "dispatch_jobs",
        &fixture.package.name,
        "id",
        &job.id.to_string(),
    )
    .unwrap();
    assert_eq!(recovered.status, Status::Completed);
    assert_eq!(recovered.error, None);
    assert!(
        jobs::claim(db, fixture.database.node, 201)
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn recovery_rejects_unknown_receipts() {
    for completion in [Completion::Command, Completion::Turn] {
        for receipt in [
            json!({"Ok":{"kind":"future_output","data":{"private":"value"}}}),
            json!({"Ok":{"kind":"plugin_transaction","data":[{"kind":"future_output","data":"ignored"}]}}),
        ] {
            let fixture = Fixture::new();
            let mut handler = fixture.setup("invalid");
            handler.callback.completion = completion;
            let db = &fixture.database.connection;
            let handler = handlers::save(db, &fixture.package, &handler).unwrap();
            let job = events::enqueue(
                db,
                Some(&fixture.package),
                &handler,
                &fixture.event("invalid"),
            )
            .unwrap();
            let (work, _) = jobs::claim(db, fixture.database.node, 200).unwrap();
            let request = encode(&work[0].request).unwrap();
            let receipt = receipt.to_string();
            db.execute(
                "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
                params![
                    &fixture.database.node.0[..],
                    job.request.to_string(),
                    request,
                    receipt
                ],
            )
            .unwrap();
            let before: Vec<u8> = db
                .query_row(
                    "SELECT body FROM dispatch_jobs WHERE id=?1",
                    [job.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                jobs::recover(db, fixture.database.node).unwrap_err().code,
                ErrorCode::Internal
            );
            let after: Vec<u8> = db
                .query_row(
                    "SELECT body FROM dispatch_jobs WHERE id=?1",
                    [job.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(after, before);
            assert!(
                jobs::claim(db, fixture.database.node, 201)
                    .unwrap()
                    .0
                    .is_empty()
            );
            let stored: (String, String, Vec<u8>) = db
                .query_row("SELECT status,result,body FROM requests", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap();
            assert_eq!(stored, ("completed".into(), receipt, request));
            assert_eq!(
                jobs::result(db, &fixture.package.name, job.id)
                    .unwrap_err()
                    .code,
                ErrorCode::Internal
            );
        }
    }
}

#[test]
fn full_queue_rolls_back_fanout_preserving_schedule() {
    let fixture = Fixture::new();
    let handler = fixture.setup("fanout");
    let db = &fixture.database.connection;
    let mut second = handler.clone();
    second.name = "second".into();
    second.revision = 0;
    handlers::save(db, &fixture.package, &second).unwrap();
    let schedule = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "fanout".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1000 },
            next_ms: None,
        },
    )
    .unwrap();
    // One free slot is insufficient for the two registered listeners.
    db.execute_batch("BEGIN").unwrap();
    let mut ids = Vec::new();
    for _ in 0..MAX_PENDING - 1 {
        ids.push(
            events::enqueue(
                db,
                Some(&fixture.package),
                &handler,
                &fixture.event("fanout"),
            )
            .unwrap()
            .id,
        );
    }
    db.execute_batch("COMMIT").unwrap();
    assert!(schedules::due(db, 1000).unwrap().is_empty());
    let count: i64 = db
        .query_row("SELECT count(*) FROM dispatch_jobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, (MAX_PENDING - 1) as i64);
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &schedule.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, Some(1000));
    let events: i64 = db
        .query_row("SELECT count(*) FROM dispatch_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 0);
    jobs::cancel(db, &fixture.package.name, ids[0]).unwrap();
    assert_eq!(
        schedules::due(db, 1001).unwrap(),
        vec![fixture.package.name.clone()]
    );
    let stored: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &schedule.id.to_string(),
    )
    .unwrap();
    assert_eq!(stored.next_ms, None);
}

#[test]
fn inactive_namespaces_do_not_starve_due_schedules() {
    let fixture = Fixture::new();
    fixture.setup("active");
    let db = &fixture.database.connection;
    for _ in 0..64 {
        schedules::save(
            db,
            "uninstalled",
            &Schedule {
                id: protocol::ScheduleId::new(),
                revision: 0,
                enabled: true,
                topic: "inactive".into(),
                payload: json!(null),
                timing: Timing::Once { at_ms: 1000 },
                next_ms: None,
            },
        )
        .unwrap();
    }
    schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "active".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1001 },
            next_ms: None,
        },
    )
    .unwrap();
    assert_eq!(
        schedules::due(db, 2000).unwrap(),
        vec![fixture.package.name.clone()]
    );
    assert_eq!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .len(),
        1
    );
}

#[test]
fn fanout_invalidates_recipient_namespaces() {
    let fixture = Fixture::new();
    let handler = fixture.setup("fanout");
    let db = &fixture.database.connection;
    let other = install(db, fixture._directory.path(), "dispatch-recipient")
        .summary
        .reference();
    let mut subscribed = handler;
    subscribed.revision = 0;
    handlers::save(db, &other, &subscribed).unwrap();
    let (output, changed) = execute(
        db,
        &fixture.package,
        &Command::Publish {
            topic: "fanout".into(),
            payload: json!(null),
        },
    )
    .unwrap();
    let protocol::Output::Dispatch(Output::Published { jobs, .. }) = output else {
        panic!("delivery expected")
    };
    assert_eq!(jobs.len(), 2);
    let Some(protocol::Event::DispatchChanged { packages }) = changed else {
        panic!("invalidation expected")
    };
    assert!(packages.contains(&fixture.package.name));
    assert!(packages.contains(&other.name));
    let page = jobs::list(db, &other.name, None, 10).unwrap();
    assert_eq!(page.jobs.len(), 1);
    assert_eq!(page.jobs[0].event.source.package, fixture.package.name);
}

#[test]
fn queries_do_not_enter_the_durable_ledger() {
    let fixture = Fixture::new();
    for action in [
        Command::ListHandlers,
        Command::ListSchedules,
        Command::ListJobs {
            before: None,
            limit: 10,
        },
        Command::ReadJob {
            id: protocol::JobId::new(),
        },
    ] {
        assert!(
            !protocol::Command::Dispatch {
                package: fixture.package.clone(),
                action
            }
            .durable()
        );
    }
    assert!(
        protocol::Command::Dispatch {
            package: fixture.package,
            action: Command::Publish {
                topic: "event".into(),
                payload: json!(null),
            }
        }
        .durable()
    );
}

pub(super) fn install(
    db: &Connection,
    root: &std::path::Path,
    name: &str,
) -> protocol::plugin::Info {
    let profile = root.join("profile");
    let source = root.join(name);
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":name,
            "extensions":{"dev.sailry.platform":{"api_version":"v1","actions":[]}}
        })
        .to_string(),
    )
    .unwrap();
    let host = crate::plugins::Host::new(Some(profile.canonicalize().unwrap()));
    let mut result = Ok(protocol::Output::Plugin(
        host.install(&source.canonicalize().unwrap(), "", name)
            .unwrap(),
    ));
    crate::store::plugins::finish(
        db,
        Some(&profile),
        &protocol::Command::InstallPlugin {
            name: name.into(),
            worktree: protocol::WorktreeId::new(),
            path: name.into(),
            expected_revision: 0,
            expected_digest: None,
        },
        &mut result,
    )
    .unwrap();
    let protocol::Output::Plugin(info) = result.unwrap() else {
        panic!("package expected")
    };
    info
}

#[test]
fn invalid_occurrences_do_not_starve_valid_schedules() {
    let fixture = Fixture::new();
    fixture.setup("valid");
    let db = &fixture.database.connection;
    let mut invalid = Vec::new();
    for index in 0..65 {
        let timing = if index % 2 == 0 {
            Timing::Every {
                anchor_ms: 0,
                interval_ms: 0,
            }
        } else {
            Timing::Weekly {
                timezone: "invalid-zone".into(),
                weekdays: vec![1],
                hour: 12,
                minute: 0,
            }
        };
        let schedule = Schedule {
            id: protocol::ScheduleId::new(),
            revision: 1,
            enabled: true,
            topic: "invalid".into(),
            payload: json!(null),
            timing,
            next_ms: Some(1000),
        };
        let body = encode(&schedule).unwrap();
        db.execute(
            "INSERT INTO dispatch_schedules(package,id,next_ms,body) VALUES(?1,?2,1000,?3)",
            params![fixture.package.name, schedule.id.to_string(), body],
        )
        .unwrap();
        invalid.push((schedule.id, body));
    }
    let valid = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "valid".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1001 },
            next_ms: None,
        },
    )
    .unwrap();
    assert_eq!(
        schedules::due(db, 2000).unwrap(),
        vec![fixture.package.name.clone()]
    );
    let page = jobs::list(db, &fixture.package.name, None, 100).unwrap();
    assert_eq!(page.jobs.len(), 1);
    assert_eq!(page.jobs[0].event.schedule, Some(valid.id));
    assert!(schedules::due(db, 2001).unwrap().is_empty());
    for (id, original) in invalid {
        let (cursor, body): (i64, Vec<u8>) = db
            .query_row(
                "SELECT next_ms,body FROM dispatch_schedules WHERE package=?1 AND id=?2",
                params![fixture.package.name, id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(cursor, 1000);
        assert_eq!(body, original);
    }
}

#[test]
fn storage_failure_preserves_the_schedule_cursor() {
    let fixture = Fixture::new();
    fixture.setup("valid");
    let db = &fixture.database.connection;
    let schedule = schedules::save(
        db,
        &fixture.package.name,
        &Schedule {
            id: protocol::ScheduleId::new(),
            revision: 0,
            enabled: true,
            topic: "valid".into(),
            payload: json!(null),
            timing: Timing::Once { at_ms: 1000 },
            next_ms: None,
        },
    )
    .unwrap();
    db.execute_batch("DROP TABLE dispatch_events").unwrap();
    assert_eq!(
        schedules::due(db, 2000).unwrap_err().code,
        ErrorCode::Internal
    );
    assert!(
        jobs::list(db, &fixture.package.name, None, 100)
            .unwrap()
            .jobs
            .is_empty()
    );
    let saved: Schedule = required(
        db,
        "dispatch_schedules",
        &fixture.package.name,
        "id",
        &schedule.id.to_string(),
    )
    .unwrap();
    assert_eq!(saved, schedule);
}

#[test]
fn backup_schedule_keeps_native_authority() {
    let fixture = Fixture::new();
    let db = &fixture.database.connection;
    let mut settings = protocol::backup::Settings::default();
    native::configure(db, &settings).unwrap();
    assert!(
        list::<Schedule>(db, "dispatch_schedules", native::OWNER)
            .unwrap()
            .is_empty()
    );
    settings.revision = 7;
    settings.schedule = Some(protocol::backup::Schedule {
        enabled: true,
        timezone: "Asia/Shanghai".into(),
        hour: 9,
        minute: 5,
    });
    native::configure(db, &settings).unwrap();
    let first: Vec<Schedule> = list(db, "dispatch_schedules", native::OWNER).unwrap();
    assert_eq!(first.len(), 1);
    let original = first[0].clone();
    let due = original.next_ms.unwrap();
    schedules::due(db, due).unwrap();
    let queued = jobs::list(db, native::OWNER, None, 10).unwrap();
    assert_eq!(queued.jobs.len(), 1);
    let id = queued.jobs[0].id;
    assert_eq!(
        jobs::cancel(db, &fixture.package.name, id)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let (work, _) = jobs::claim(db, fixture.database.node, due).unwrap();
    assert_eq!(work.len(), 1);
    assert!(work[0].request.plugin.is_none());
    assert_eq!(
        work[0].request.command,
        protocol::Command::ExportBackup {
            expected_revision: 7
        }
    );
    assert_eq!(work[0].request.id, queued.jobs[0].request);
    assert!(
        jobs::package(
            db,
            fixture.database.node,
            fixture.database.node,
            &work[0].request
        )
        .unwrap()
        .is_none()
    );
    let mut handler: Handler = required(
        db,
        "dispatch_handlers",
        native::OWNER,
        "name",
        "profile-backup",
    )
    .unwrap();
    handler.name = "borrowed".into();
    assert!(handlers::save(db, &fixture.package, &handler).is_err());
    settings.revision += 1;
    settings.schedule.as_mut().unwrap().minute = 6;
    native::configure(db, &settings).unwrap();
    let changed: Vec<Schedule> = list(db, "dispatch_schedules", native::OWNER).unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].id, original.id);
    assert_ne!(changed[0].timing, original.timing);
    settings.schedule.as_mut().unwrap().enabled = false;
    native::configure(db, &settings).unwrap();
    assert!(!list::<Schedule>(db, "dispatch_schedules", native::OWNER).unwrap()[0].enabled);
}
