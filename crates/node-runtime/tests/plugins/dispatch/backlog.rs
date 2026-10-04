use super::*;
use std::collections::{HashMap, HashSet};

async fn page(client: &Client, context: &Context, before: Option<u64>) -> jobs::Page {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let query = client
                .prepare(Command::Dispatch {
                    package: context.package.clone(),
                    action: jobs::Command::ListJobs { before, limit: 100 },
                })
                .with_plugin(context.clone());
            match client.execute(query).await {
                Ok(Output::Dispatch(jobs::Output::Jobs(page))) => return page,
                Err(error) if error.code == ErrorCode::Busy => {
                    // Read-only observation may wait while internal admissions drain.
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                output => panic!("dispatch page expected: {output:?}"),
            }
        }
    })
    .await
    .unwrap()
}

async fn completed(client: &Client, context: &Context, count: usize) -> Vec<jobs::Job> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let mut before = None;
            let mut records = Vec::new();
            loop {
                let page = page(client, context, before).await;
                records.extend(page.jobs);
                before = page.next_before;
                if before.is_none() {
                    break;
                }
            }
            assert_eq!(records.len(), count);
            assert!(
                records.iter().all(|job| matches!(
                    job.status,
                    jobs::Status::Queued | jobs::Status::Running | jobs::Status::Completed
                )),
                "{records:?}"
            );
            if records
                .iter()
                .all(|job| job.status == jobs::Status::Completed)
            {
                return records;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn fanout_drains_writes_and_reads_without_losing_admissions() {
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
        let mut handlers = Vec::new();
        let mut expected = HashMap::new();
        for index in 0..192 {
            let name = format!("callback-{index}");
            expected.insert(name.clone(), json!(index));
            let jobs::Output::Handler(handler) = run(
                &client,
                &context,
                jobs::Command::SaveHandler(jobs::Handler {
                    name: name.clone(),
                    revision: 0,
                    enabled: true,
                    source: jobs::Source {
                        package: context.package.name.clone(),
                        topic: "fanout".into(),
                    },
                    queue: "same-group".into(),
                    callback: jobs::Callback {
                        scope: jobs::Scope::default(),
                        completion: jobs::Completion::Command,
                        command: Box::new(Command::WritePluginValue {
                            key: name,
                            value: json!(index),
                            expected_revision: 0,
                        }),
                        bindings: BTreeMap::new(),
                    },
                }),
            )
            .await
            else {
                panic!("handler expected")
            };
            handlers.push(handler);
        }
        let publish = client
            .prepare(Command::Dispatch {
                package: context.package.clone(),
                action: jobs::Command::Publish {
                    topic: "fanout".into(),
                    payload: json!(null),
                },
            })
            .with_plugin(context.clone());
        let published = client.execute(publish.clone()).await.unwrap();
        let Output::Dispatch(jobs::Output::Published { event, jobs: ids }) = &published else {
            panic!("published event expected")
        };
        assert_eq!(ids.len(), 192);
        let writes = completed(&client, &context, 192).await;
        assert_eq!(
            writes.iter().map(|job| job.id).collect::<HashSet<_>>(),
            ids.iter().copied().collect()
        );
        assert_eq!(
            writes
                .iter()
                .map(|job| job.request)
                .collect::<HashSet<_>>()
                .len(),
            192
        );
        for job in &writes {
            assert_eq!(job.event.id, *event);
            assert_eq!(job.queue, "same-group");
            let result = run(&client, &context, jobs::Command::ReadResult { id: job.id }).await;
            assert_eq!(
                run(&client, &context, jobs::Command::ReadResult { id: job.id }).await,
                result
            );
            let jobs::Output::Result(RequestOutcome::Completed(output)) = result else {
                panic!("stored original receipt expected")
            };
            let Ok(Output::PluginValue(value)) = *output else {
                panic!("stored value expected")
            };
            assert_eq!(value.key, job.handler);
            assert_eq!(value.value, expected[&job.handler]);
            assert_eq!(value.revision, 1);
        }
        assert_eq!(client.execute(publish).await.unwrap(), published);
        assert_eq!(completed(&client, &context, 192).await.len(), 192);
        for mut handler in handlers {
            handler.callback.command = Box::new(Command::ReadPluginValue {
                key: handler.name.clone(),
            });
            run(&client, &context, jobs::Command::SaveHandler(handler)).await;
        }
        run(
            &client,
            &context,
            jobs::Command::Publish {
                topic: "fanout".into(),
                payload: json!(null),
            },
        )
        .await;
        let records = completed(&client, &context, 384).await;
        let original = ids.iter().copied().collect::<HashSet<_>>();
        for job in records.iter().filter(|job| !original.contains(&job.id)) {
            assert_eq!(
                run(&client, &context, jobs::Command::ReadResult { id: job.id }).await,
                jobs::Output::Result(RequestOutcome::NotAdmitted)
            );
        }
        for (key, expected) in expected {
            let Output::PluginValue(value) = client
                .execute(
                    client
                        .prepare(Command::ReadPluginValue { key })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap()
            else {
                panic!("value expected")
            };
            assert_eq!(value.value, expected);
            assert_eq!(value.revision, 1);
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
