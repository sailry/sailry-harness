use super::*;
use crate::store::database::storage_error;
use futures::future::join_all;
use sailry_link::{Link, NetworkScope};
use tokio::sync::oneshot;
use turso::transaction::TransactionBehavior;

#[tokio::test]
async fn reads_bypass_uncommitted_writer() {
    for remote in [false, true] {
        let (directory, node, local) = node().await;
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            local
        };
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config: config("snapshot-read"),
            }))
            .await
            .unwrap();
        let expected = snapshot(&client).await;
        let identity = node.id();
        let requests = (0..160)
            .map(|_| client.prepare(Command::Snapshot))
            .collect::<Vec<_>>();
        let (entered, ready) = oneshot::channel();
        let (release, released) = oneshot::channel::<()>();
        let writer = node.with_storage(move |connection| {
            Box::pin(async move {
                let transaction = connection
                    .transaction_with_behavior(TransactionBehavior::Concurrent)
                    .await
                    .map_err(storage_error)?;
                let changed = transaction
                    .execute("UPDATE node SET identity=identity WHERE singleton=1", ())
                    .await
                    .map_err(storage_error)?;
                let _ = entered.send(());
                let _ = released.await;
                transaction.rollback().await.map_err(storage_error)?;
                Ok(changed)
            })
        });
        let reads = async {
            let entered = tokio::time::timeout(Duration::from_secs(20), ready).await;
            let results = if matches!(&entered, Ok(Ok(()))) {
                Some(
                    tokio::time::timeout(
                        Duration::from_secs(20),
                        join_all(requests.into_iter().map(|request| client.execute(request))),
                    )
                    .await,
                )
            } else {
                None
            };
            // Dropping the sender releases the writer on success and timeout alike.
            drop(release);
            (entered, results)
        };
        let (writer, (entered, results)) = tokio::join!(writer, reads);
        let closed = controller.close().await;
        let shutdown = node.shutdown().await;
        entered
            .expect("writer fixture did not enter its transaction")
            .expect("writer fixture exited before its update");
        assert_eq!(writer.expect("writer fixture failed"), 1);
        let results = results
            .expect("reader batch did not start")
            .expect("snapshots waited for the uncommitted writer");
        assert_eq!(results.len(), 160);
        for result in results {
            let Output::Snapshot(actual) = result.expect("snapshot request failed") else {
                panic!("snapshot expected");
            };
            assert_eq!(actual.node, identity);
            assert_eq!(actual.defaults, expected.defaults);
            assert!(actual.cursor >= expected.cursor);
        }
        closed.unwrap();
        shutdown.unwrap();
    }
}

#[tokio::test]
async fn preserves_parallel_publications() {
    for remote in [false, true] {
        let (directory, node, local) = node().await;
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            local
        };
        let project = project(&client, &directory).await;
        for _ in 0..8 {
            let output = client
                .execute(client.prepare(Command::CreateSession {
                    project: Some(project.id),
                    worktree: None,
                    config: Some(config("publication")),
                }))
                .await
                .unwrap();
            assert!(matches!(output, Output::Session(_)));
        }
        let initial = snapshot(&client).await;
        assert_eq!(initial.sessions.len(), 8);
        let check = |snapshot: &Snapshot| {
            assert_eq!(snapshot.sessions.len(), initial.sessions.len());
            let mut changed = 0;
            for original in &initial.sessions {
                let current = snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == original.id)
                    .unwrap();
                if current.revision == original.revision {
                    assert_eq!(current, original);
                } else {
                    assert_eq!(current.revision, original.revision + 1);
                    assert_eq!(current.activity.title, "Published rename");
                    changed += 1;
                }
            }
            assert_eq!(snapshot.cursor, initial.cursor + changed);
        };
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        assert_eq!(projection.snapshot(), Some(&initial));
        let requests = initial
            .sessions
            .iter()
            .map(|session| {
                client.prepare(Command::RenameSession {
                    session: session.id,
                    expected_revision: session.revision,
                    title: "Published rename".into(),
                })
            })
            .collect::<Vec<_>>();
        let writing = join_all(requests.into_iter().map(|request| client.execute(request)));
        let reading = async {
            for _ in 0..16 {
                check(&snapshot(&client).await);
                tokio::task::yield_now().await;
            }
        };
        let (results, (), raced) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::join!(writing, reading, client.subscribe())
        })
        .await
        .expect("parallel publication fixture timed out");
        assert_eq!(results.len(), initial.sessions.len());
        for result in results {
            let Output::Session(session) = result.unwrap() else {
                panic!("renamed session expected");
            };
            assert_eq!(session.activity.title, "Published rename");
        }
        let committed = snapshot(&client).await;
        check(&committed);
        assert_eq!(
            committed.cursor,
            initial.cursor + initial.sessions.len() as u64
        );
        let mut raced = raced.unwrap();
        let mut raced_projection = Projection::new(node.id(), 1);
        assert_eq!(
            raced_projection
                .apply(1, raced.next().await.unwrap())
                .unwrap(),
            Apply::Applied
        );
        check(raced_projection.snapshot().unwrap());
        for (updates, projection) in [
            (&mut updates, &mut projection),
            (&mut raced, &mut raced_projection),
        ] {
            while projection.snapshot().unwrap().cursor < committed.cursor {
                let cursor = projection.snapshot().unwrap().cursor;
                let update = tokio::time::timeout(Duration::from_secs(5), updates.next())
                    .await
                    .expect("committed event was not delivered")
                    .unwrap();
                let Update::Event(envelope) = &update else {
                    panic!("durable event expected");
                };
                assert_eq!(envelope.cursor, cursor + 1);
                let Event::SessionChanged(session) = &envelope.event else {
                    panic!("renamed session event expected");
                };
                assert_eq!(session.activity.title, "Published rename");
                assert_eq!(projection.apply(1, update).unwrap(), Apply::Applied);
            }
            assert_eq!(projection.snapshot(), Some(&committed));
        }
        drop(updates);
        drop(raced);
        let session = &committed.sessions[0];
        let Output::Session(offline) = client
            .execute(client.prepare(Command::RenameSession {
                session: session.id,
                expected_revision: session.revision,
                title: "While disconnected".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("renamed session expected");
        };
        let recovered = snapshot(&client).await;
        assert_eq!(recovered.cursor, committed.cursor + 1);
        projection.reconnect(2).unwrap();
        let mut resumed = client.subscribe().await.unwrap();
        assert_eq!(
            projection.apply(2, resumed.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        assert_eq!(projection.snapshot(), Some(&recovered));
        assert!(
            projection
                .snapshot()
                .unwrap()
                .sessions
                .iter()
                .any(|session| session == &offline)
        );
        client
            .execute(client.prepare(Command::RenameSession {
                session: offline.id,
                expected_revision: offline.revision,
                title: "After reconnect".into(),
            }))
            .await
            .unwrap();
        let update = tokio::time::timeout(Duration::from_secs(5), resumed.next())
            .await
            .expect("event after reconnect was not delivered")
            .unwrap();
        let Update::Event(envelope) = &update else {
            panic!("durable event expected");
        };
        assert_eq!(envelope.cursor, recovered.cursor + 1);
        assert_eq!(projection.apply(2, update).unwrap(), Apply::Applied);
        assert_eq!(projection.snapshot(), Some(&snapshot(&client).await));
        drop(resumed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn reads_bypass_exhausted_write_budget() {
    for remote in [false, true] {
        let (directory, node, local) = node().await;
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            local
        };
        let expected = snapshot(&client).await;
        let writes = node.ingress.storage.get().unwrap().writes.clone();
        let permits = writes
            .clone()
            .acquire_many_owned(crate::store::database::WRITE_CONCURRENCY as u32)
            .await
            .unwrap();
        let config = config("after-budget-release");
        let mut writer = Box::pin(client.dispatch(client.prepare(Command::SetDefaults {
            expected_revision: expected.defaults.revision,
            config: config.clone(),
        })));
        let waiting = tokio::time::timeout(Duration::from_millis(20), &mut writer).await;
        let pending = waiting.is_err();
        let requests = (0..160)
            .map(|_| client.prepare(Command::Snapshot))
            .collect::<Vec<_>>();
        let reads = tokio::time::timeout(
            Duration::from_secs(20),
            join_all(requests.into_iter().map(|request| client.execute(request))),
        )
        .await;
        let available = writes.available_permits();
        // Always release admission before awaiting the same writer's completion.
        drop(permits);
        let completed = async {
            let admission = match waiting {
                Ok(result) => result,
                Err(_) => tokio::time::timeout(Duration::from_secs(20), &mut writer)
                    .await
                    .map_err(|error| error.to_string())?,
            }
            .map_err(|error| error.to_string())?;
            let output = tokio::time::timeout(Duration::from_secs(20), admission.completion)
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((admission.receipt, output))
        }
        .await;
        drop(writer);
        let changed = tokio::time::timeout(
            Duration::from_secs(20),
            client.execute(client.prepare(Command::Snapshot)),
        )
        .await;
        let closed = controller.close().await;
        let shutdown = node.shutdown().await;
        assert!(pending, "durable admission bypassed the exhausted budget");
        assert_eq!(available, 0);
        let reads = reads.expect("snapshots waited for the exhausted write budget");
        assert_eq!(reads.len(), 160);
        for result in reads {
            let Output::Snapshot(actual) = result.expect("snapshot request failed") else {
                panic!("snapshot expected");
            };
            assert_eq!(actual, expected);
        }
        let (receipt, output) = completed.expect("writer did not finish after budget release");
        assert!(receipt.durable);
        let Output::Defaults(defaults) = output else {
            panic!("updated defaults expected");
        };
        assert_eq!(defaults.revision, expected.defaults.revision + 1);
        assert_eq!(defaults.config, Some(config));
        let Output::Snapshot(actual) = changed
            .expect("snapshot after budget release timed out")
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert_eq!(actual.cursor, expected.cursor + 1);
        assert_eq!(actual.defaults, defaults);
        closed.unwrap();
        shutdown.unwrap();
    }
}

#[tokio::test]
async fn preserves_parallel_request_identity() {
    for remote in [false, true] {
        let (directory, node, local) = node().await;
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            local
        };
        let initial = snapshot(&client).await;
        let request = client.prepare(Command::SetDefaults {
            expected_revision: initial.defaults.revision,
            config: config("parallel-replay"),
        });
        let results = join_all((0..16).map(|_| client.execute(request.clone()))).await;
        let output = results.first().unwrap().as_ref().unwrap().clone();
        for result in results {
            assert_eq!(result.unwrap(), output);
        }
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            sailry_protocol::RequestOutcome::Completed(Box::new(Ok(output.clone())))
        );
        let Output::Defaults(defaults) = output else {
            panic!("defaults expected")
        };
        assert_eq!(defaults.revision, initial.defaults.revision + 1);
        let committed = snapshot(&client).await;
        assert_eq!(committed.defaults, defaults);
        assert_eq!(committed.cursor, initial.cursor + 1);
        let stale = client.prepare(Command::SetDefaults {
            expected_revision: initial.defaults.revision,
            config: config("stale-replay"),
        });
        let error = client.execute(stale.clone()).await.unwrap_err();
        assert_eq!(error.code, sailry_protocol::ErrorCode::RevisionConflict);
        assert_eq!(client.execute(stale.clone()).await.unwrap_err(), error);
        assert_eq!(
            client.outcome(&stale).await.unwrap(),
            sailry_protocol::RequestOutcome::Completed(Box::new(Err(error)))
        );
        assert_eq!(snapshot(&client).await, committed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
