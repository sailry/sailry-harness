use super::*;
use sailry_link::Subscription;
use std::time::Duration;

fn expected(sessions: &[Session]) -> Vec<SessionRevision> {
    sessions
        .iter()
        .filter(|session| session.delegation.is_none())
        .map(|session| SessionRevision {
            session: session.id,
            revision: session.revision,
        })
        .collect()
}

async fn synchronized(
    client: &Client,
    updates: &mut dyn Subscription,
    projection: &mut Projection,
) -> Snapshot {
    let current = snapshot(client).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while projection.snapshot().unwrap().cursor < current.cursor {
            projection.apply(1, updates.next().await.unwrap()).unwrap();
        }
    })
    .await
    .expect("session batch publication deadline");
    assert_eq!(projection.snapshot().unwrap().sessions, current.sessions);
    current
}

#[tokio::test]
async fn preserves_atomic_visibility_and_resources() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("keep.txt"), "project content").unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let provider = ProviderId::new();
        let credential = CredentialId::new();
        let secret = "private-batch-fixture-key";
        client
            .execute(client.prepare(Command::PutCredential {
                id: credential,
                provider,
                expected_revision: 0,
                secret: Secret::new(secret.into()),
                expires_at_ms: None,
            }))
            .await
            .unwrap();
        let credentials = client
            .execute(client.prepare(Command::ListCredentials))
            .await
            .unwrap();
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Batch fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let config = SessionConfig {
            features: Default::default(),
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            options: Default::default(),
            permission: Permission::Ask,
            credential: None,
        };
        let mut sessions = Vec::new();
        for project in [Some(project.id), None, Some(project.id)] {
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project,
                    worktree: None,
                    config: Some(config.clone()),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            sessions.push(session);
        }
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        let before_archive = snapshot(&client).await;
        let mut stale = expected(&sessions);
        stale[2].revision += 1;
        assert_eq!(
            client
                .execute(client.prepare(Command::ArchiveSessions { expected: stale }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let rejected = snapshot(&client).await;
        assert_eq!(rejected.sessions, before_archive.sessions);
        assert_eq!(rejected.cursor, before_archive.cursor);
        let duplicate = vec![expected(&sessions)[0]; 2];
        assert_eq!(
            client
                .execute(client.prepare(Command::ArchiveSessions {
                    expected: duplicate
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let archive = client.prepare(Command::ArchiveSessions {
            expected: expected(&snapshot(&client).await.sessions),
        });
        let Output::SessionsArchived(archived) = client.execute(archive.clone()).await.unwrap()
        else {
            panic!("archived sessions expected")
        };
        assert_eq!(archived.len(), 3);
        assert!(
            archived
                .iter()
                .all(|session| session.archived && session.revision == 2)
        );
        assert_eq!(
            client.execute(archive).await.unwrap(),
            Output::SessionsArchived(archived.clone())
        );
        synchronized(&client, &mut *updates, &mut projection).await;
        let Output::Session(restored) = client
            .execute(client.prepare(Command::SetSessionArchived {
                session: sessions[0].id,
                expected_revision: 2,
                archived: false,
            }))
            .await
            .unwrap()
        else {
            panic!("restored session expected")
        };
        assert!(!restored.archived);
        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::QueueTurn {
                session: restored.id,
                expected_revision: restored.revision,
                message: "Keep this queued".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let before_remove = snapshot(&client).await;
        let mut selected = expected(&before_remove.sessions);
        // A later busy session rolls back earlier removals in the same command.
        selected.sort_by_key(|item| item.session == restored.id);
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveSessions { expected: selected }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        let rejected = snapshot(&client).await;
        assert_eq!(rejected.sessions, before_remove.sessions);
        assert_eq!(rejected.cursor, before_remove.cursor);
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "project content"
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: queued.id }))
            .await
            .unwrap();
        let current = synchronized(&client, &mut *updates, &mut projection).await;
        let mut unadmitted = Vec::new();
        for command in [
            Command::ArchiveSessions {
                expected: expected(&current.sessions),
            },
            Command::RemoveSessions {
                expected: expected(&current.sessions),
            },
        ] {
            // A failed receipt write rolls back the database-only mutation and
            // admission together, so its original identity can be submitted safely.
            node.with_storage(|db| Box::pin(async move {
                db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").await.map_err(crate::database_fixture::fault)
            })).await.unwrap();
            let request = client.prepare(command);
            assert!(client.execute(request.clone()).await.is_err());
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::NotAdmitted
            );
            node.with_storage(|db| {
                Box::pin(async move {
                    db.execute_batch("DROP TRIGGER lose_result")
                        .await
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
            let unchanged = snapshot(&client).await;
            assert_eq!(unchanged.sessions, current.sessions);
            assert_eq!(unchanged.cursor, current.cursor);
            unadmitted.push(request);
        }
        let remove = unadmitted.pop().unwrap();
        assert_eq!(
            client.outcome(&remove).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        let result = client.execute(remove.clone()).await.unwrap();
        let Output::SessionsRemoved(removed) = &result else {
            panic!("removed sessions expected")
        };
        assert_eq!(removed.len(), 3);
        assert_eq!(client.execute(remove.clone()).await.unwrap(), result);
        assert_eq!(
            client.outcome(&remove).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(result.clone())))
        );
        let other = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        assert_eq!(
            other.outcome(&remove).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        let current = synchronized(&client, &mut *updates, &mut projection).await;
        assert!(current.sessions.is_empty());
        assert!(current.turns.is_empty());
        assert_eq!(current.projects, vec![project.clone()]);
        assert_eq!(
            client
                .execute(client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            credentials
        );
        assert_eq!(
            node.resolve_credential(
                CredentialRef {
                    node: node.id(),
                    id: credential
                },
                provider
            )
            .await
            .unwrap()
            .expose(),
            secret
        );
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "project content"
        );
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert!(snapshot(&client).await.sessions.is_empty());
        assert_eq!(client.execute(remove).await.unwrap(), result);
        for request in unadmitted {
            assert_eq!(
                client.outcome(&request).await.unwrap(),
                RequestOutcome::NotAdmitted
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            credentials
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
