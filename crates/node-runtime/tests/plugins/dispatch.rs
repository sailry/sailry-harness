use super::*;
use sailry_protocol::{
    dispatch as jobs,
    plugin::{Action, Context},
};
use serde_json::json;
use std::collections::BTreeMap;

#[path = "dispatch/backlog.rs"]
mod backlog;

pub(super) async fn run(client: &Client, context: &Context, action: jobs::Command) -> jobs::Output {
    let Output::Dispatch(output) = client
        .execute(
            client
                .prepare(Command::Dispatch {
                    package: context.package.clone(),
                    action,
                })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("dispatch output expected")
    };
    output
}

async fn setup(client: &Client, context: &Context) {
    run(
        client,
        context,
        jobs::Command::SaveHandler(jobs::Handler {
            name: "receive".into(),
            revision: 0,
            enabled: true,
            source: jobs::Source {
                package: context.package.name.clone(),
                topic: "message".into(),
            },
            queue: "events".into(),
            callback: jobs::Callback {
                scope: jobs::Scope::default(),
                completion: jobs::Completion::Command,
                command: Box::new(Command::WritePluginValue {
                    key: "placeholder".into(),
                    value: json!(null),
                    expected_revision: 0,
                }),
                bindings: BTreeMap::from([
                    ("/data/key".into(), "/payload/key".into()),
                    ("/data/value".into(), "/payload/value".into()),
                ]),
            },
        }),
    )
    .await;
}

async fn complete(client: &Client, context: &Context) -> jobs::Job {
    tokio::time::timeout(Duration::from_secs(10), async {
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
                assert_eq!(
                    job.status,
                    jobs::Status::Completed,
                    "callback failed: {:?}",
                    job.error
                );
                return job.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn local_and_remote_event_delivery() {
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
        let request = client
            .prepare(Command::Dispatch {
                package: context.package.clone(),
                action: jobs::Command::Publish {
                    topic: "message".into(),
                    payload: json!({"key":"received", "value":"event payload"}),
                },
            })
            .with_plugin(context.clone());
        let response = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), response);
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
        let Output::Dispatch(jobs::Output::Published { jobs, .. }) = response else {
            panic!("published event expected")
        };
        assert_eq!(jobs, vec![page.jobs[0].id]);
        complete(&client, &context).await;
        let Output::PluginValue(entry) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "received".into(),
                    })
                    .with_plugin(context),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(entry.value, json!("event payload"));
        assert_eq!(entry.revision, 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn scheduled_delivery_survives_restart() {
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
        let id = ScheduleId::new();
        run(
            &client,
            &context,
            jobs::Command::SaveSchedule(jobs::Schedule {
                id,
                revision: 0,
                enabled: false,
                topic: "message".into(),
                payload: json!({"key":"scheduled", "value":"retained"}),
                timing: jobs::Timing::Once {
                    at_ms: chrono::Utc::now().timestamp_millis() + 100,
                },
                next_ms: None,
            }),
        )
        .await;
        assert!(
            matches!(run(&client,&context,jobs::Command::ListJobs {before:None,limit:100}).await,
            jobs::Output::Jobs(page) if page.jobs.is_empty())
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let jobs::Output::Schedules(mut schedules) =
            run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules.len(), 1);
        let mut schedule = schedules.pop().unwrap();
        assert_eq!(schedule.id, id);
        assert!(!schedule.enabled);
        schedule.enabled = true;
        run(&client, &context, jobs::Command::SaveSchedule(schedule)).await;
        let job = complete(&client, &context).await;
        assert_eq!(job.event.schedule, Some(id));
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
        let jobs::Output::Schedules(schedules) =
            run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules[0].next_ms, None);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn callbacks_cannot_expand_resource_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Dispatch, Action::ReadStorage],
        )
        .await;
        let handler = jobs::Handler {
            name: "foreign".into(),
            revision: 0,
            enabled: true,
            source: jobs::Source {
                package: context.package.name.clone(),
                topic: "message".into(),
            },
            queue: "events".into(),
            callback: jobs::Callback {
                completion: jobs::Completion::Command,
                scope: jobs::Scope {
                    worktree: Some(WorktreeId::new()),
                    session: None,
                },
                command: Box::new(Command::ReadPluginValue {
                    key: "private".into(),
                }),
                bindings: BTreeMap::new(),
            },
        };
        let request = client
            .prepare(Command::Dispatch {
                package: context.package.clone(),
                action: jobs::Command::SaveHandler(handler),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let mut other = context.package.clone();
        other.name = "other".into();
        let request = client
            .prepare(Command::Dispatch {
                package: other,
                action: jobs::Command::ListHandlers,
            })
            .with_plugin(context);
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn callbacks_recheck_declared_permissions() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Dispatch, Action::ReadStorage],
        )
        .await;
        setup(&client, &context).await;
        let jobs::Output::Job(job) = run(
            &client,
            &context,
            jobs::Command::Enqueue {
                handler: "receive".into(),
                payload: json!({"key":"forbidden", "value":"no write grant"}),
            },
        )
        .await
        else {
            panic!("job expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let jobs::Output::Job(current) =
                    run(&client, &context, jobs::Command::ReadJob { id: job.id }).await
                else {
                    panic!("job expected")
                };
                if !matches!(current.status, jobs::Status::Queued | jobs::Status::Running) {
                    assert_eq!(current.status, jobs::Status::Failed);
                    assert_eq!(current.error.unwrap().code, ErrorCode::PermissionDenied);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let Output::PluginValue(entry) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue {
                        key: "forbidden".into(),
                    })
                    .with_plugin(context),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(entry.revision, 0);
        assert_eq!(entry.value, json!(null));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
