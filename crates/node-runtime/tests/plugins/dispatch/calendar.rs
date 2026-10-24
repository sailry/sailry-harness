use super::*;
use chrono::{Datelike, Timelike, Utc};

fn timing() -> (jobs::Timing, i64) {
    let now = Utc::now().timestamp_millis();
    let scheduled = (now.div_euclid(60_000) - 1) * 60_000;
    let local = chrono::DateTime::<Utc>::from_timestamp_millis(scheduled).unwrap();
    (
        jobs::Timing::Weekly {
            timezone: "UTC".into(),
            weekdays: vec![local.weekday().number_from_monday() as u8],
            hour: local.hour() as u8,
            minute: local.minute() as u8,
        },
        scheduled,
    )
}

async fn wait(client: &Client, context: &Context) -> jobs::Job {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let jobs::Output::Jobs(page) = run(
                client,
                context,
                jobs::Command::ListJobs {
                    before: None,
                    limit: 100,
                },
            )
            .await
            else {
                panic!("jobs expected")
            };
            if let Some(job) = page.jobs.first()
                && !matches!(job.status, jobs::Status::Queued | jobs::Status::Running)
            {
                assert_eq!(job.status, jobs::Status::Completed, "{:?}", job.error);
                return job.clone();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .expect("calendar delivery deadline")
}

#[tokio::test]
async fn delivers_overdue_calendar_after_restart() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Dispatch, Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        setup(&client, &context).await;
        let (timing, scheduled) = timing();
        let id = ScheduleId::new();
        let jobs::Output::Schedule(saved) = run(
            &client,
            &context,
            jobs::Command::SaveSchedule(jobs::Schedule {
                id,
                revision: 0,
                enabled: true,
                topic: "message".into(),
                payload: json!({"key":"calendar", "value":"retained"}),
                timing,
                next_ms: Some(1),
            }),
        )
        .await
        else {
            panic!("schedule expected")
        };
        assert_eq!(saved.next_ms, Some(scheduled + 7 * 86_400_000));
        drop(client);
        node.shutdown().await.unwrap();

        // Model downtime in this isolated profile while retaining the exact
        // weekly rule. The real restarted worker must claim and deliver it.
        let (database_handle, database) = crate::plugins::database_fixture::open(
            &(directory.path().join("node/storage/node.turso")),
            false,
        )
        .await;
        let (cursor, body): (Option<i64>, Vec<u8>) = (async {
            let mut statement = database
                .prepare("SELECT next_ms,body FROM dispatch_schedules WHERE package=?1 AND id=?2")
                .await?;
            let row = statement
                .query_row(turso::params![
                    context.package.name.as_str(),
                    id.to_string()
                ])
                .await?;
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(cursor, saved.next_ms);
        let mut persisted: jobs::Schedule = serde_json::from_slice(&body).unwrap();
        assert_eq!(persisted, saved);
        persisted.next_ms = Some(scheduled);
        assert_eq!(
            database
                .execute(
                    "UPDATE dispatch_schedules SET next_ms=?1,body=?2 WHERE package=?3 AND id=?4",
                    turso::params![
                        scheduled,
                        serde_json::to_vec(&persisted).unwrap(),
                        context.package.name.as_str(),
                        id.to_string()
                    ],
                )
                .await
                .unwrap(),
            1
        );
        crate::plugins::database_fixture::close(database_handle, database).await;

        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let delivered = wait(&client, &context).await;
        assert_eq!(delivered.event.schedule, Some(id));
        assert_eq!(delivered.event.scheduled_ms, Some(scheduled));
        assert!(delivered.event.timestamp_ms >= scheduled);
        let Output::PluginValue(entry) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "calendar".into(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(entry.value, json!("retained"));
        assert_eq!(entry.revision, 1);
        let jobs::Output::Schedules(schedules) =
            run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules[0].next_ms, Some(scheduled + 7 * 86_400_000));
        let jobs::Output::Jobs(page) = run(
            &client,
            &context,
            jobs::Command::ListJobs {
                before: None,
                limit: 100,
            },
        )
        .await
        else {
            panic!("jobs expected")
        };
        assert_eq!(page.jobs.len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_schedule_does_not_block_other_deliveries() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Dispatch, Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        setup(&client, &context).await;
        let mut records = Vec::new();
        for topic in ["invalid", "message"] {
            let jobs::Output::Schedule(schedule) = run(
                &client,
                &context,
                jobs::Command::SaveSchedule(jobs::Schedule {
                    id: ScheduleId::new(),
                    revision: 0,
                    enabled: false,
                    topic: topic.into(),
                    payload: json!({"key":"isolation","value":"once"}),
                    timing: jobs::Timing::Once { at_ms: 1000 },
                    next_ms: None,
                }),
            )
            .await
            else {
                panic!("schedule expected")
            };
            records.push(schedule);
        }
        drop(client);
        node.shutdown().await.unwrap();
        let (database_handle, database) = crate::plugins::database_fixture::open(
            &(directory.path().join("node/storage/node.turso")),
            false,
        )
        .await;
        let mut originals = Vec::new();
        for (index, schedule) in records.iter_mut().enumerate() {
            schedule.enabled = true;
            if index == 0 {
                schedule.timing = jobs::Timing::Every {
                    anchor_ms: 1000,
                    interval_ms: 0,
                };
            }
            let body = serde_json::to_vec(schedule).unwrap();
            database
                .execute(
                    "UPDATE dispatch_schedules SET next_ms=1000,body=?1 WHERE package=?2 AND id=?3",
                    turso::params![
                        body.as_slice(),
                        context.package.name.as_str(),
                        schedule.id.to_string()
                    ],
                )
                .await
                .unwrap();
            originals.push(body);
        }
        crate::plugins::database_fixture::close(database_handle, database).await;
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let delivered = complete(&client, &context).await;
        assert_eq!(delivered.event.schedule, Some(records[1].id));
        let jobs::Output::Schedules(saved) =
            run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(
            saved.iter().find(|schedule| schedule.id == records[0].id),
            Some(&records[0])
        );
        assert!(
            saved
                .iter()
                .find(|schedule| schedule.id == records[1].id)
                .unwrap()
                .next_ms
                .is_none()
        );
        let Output::PluginValue(value) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "isolation".into(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(value.value, json!("once"));
        assert_eq!(value.revision, 1);
        let jobs::Output::Jobs(page) = run(
            &client,
            &context,
            jobs::Command::ListJobs {
                before: None,
                limit: 100,
            },
        )
        .await
        else {
            panic!("jobs expected")
        };
        assert_eq!(page.jobs.len(), 1);
        node.shutdown().await.unwrap();
        let (database_handle, database) = crate::plugins::database_fixture::open(
            &(directory.path().join("node/storage/node.turso")),
            false,
        )
        .await;
        let body: Vec<u8> = (async {
            let mut statement = database
                .prepare("SELECT body FROM dispatch_schedules WHERE package=?1 AND id=?2")
                .await?;
            let row = statement
                .query_row(turso::params![
                    context.package.name.as_str(),
                    records[0].id.to_string()
                ])
                .await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(body, originals[0]);
        crate::plugins::database_fixture::close(database_handle, database).await;
        controller.close().await.unwrap();
    }
}
