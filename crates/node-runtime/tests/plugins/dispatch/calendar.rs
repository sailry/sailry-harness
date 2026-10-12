use super::*;
use chrono::{Datelike, Timelike, Utc};

fn timing() -> (jobs::Timing, i64) {
    let now = Utc::now().timestamp_millis();
    let mut scheduled = (now.div_euclid(60_000) + 1) * 60_000;
    if scheduled - now < 10_000 {
        scheduled += 60_000;
    }
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
async fn delivery_survives_restart() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
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
        assert_eq!(saved.next_ms, Some(scheduled));
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
