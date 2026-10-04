use super::*;
use sailry_link::Subscription;
use std::time::Duration;

async fn ordered(
    client: &Client,
    updates: &mut dyn Subscription,
    projection: &mut Projection,
    expected: &[SessionId],
) {
    let current = snapshot(client).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while projection.snapshot().unwrap().cursor < current.cursor {
            projection.apply(1, updates.next().await.unwrap()).unwrap();
        }
    })
    .await
    .expect("session order update deadline");
    assert_eq!(projection.snapshot().unwrap().sessions, current.sessions);
    assert_eq!(
        current
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        expected
    );
}

#[tokio::test]
async fn restores_snapshot_order() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
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
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Order fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        let config = SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        };
        let mut sessions = Vec::new();
        for _ in 0..3 {
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: Some(project.id),
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
        let [first, second, third] = std::array::from_fn(|index| sessions[index].id);
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;

        let change = client.prepare(Command::SetSessionConfig {
            session: first,
            expected_revision: 1,
            config: SessionConfig {
                effort: Effort::High,
                ..config.clone()
            },
        });
        client.execute(change.clone()).await.unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;

        let Output::QueuedTurn(queued) = client
            .execute(client.prepare(Command::QueueTurn {
                session: second,
                expected_revision: 1,
                message: "Queued activity".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;
        client.execute(change).await.unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;

        client
            .execute(client.prepare(Command::SetSessionConfig {
                session: third,
                expected_revision: 1,
                config: SessionConfig {
                    effort: Effort::High,
                    ..config
                },
            }))
            .await
            .unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;
        client
            .execute(client.prepare(Command::StopTurn { turn: queued.id }))
            .await
            .unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[third, second, first],
        )
        .await;

        let reorder = client.prepare(Command::SetSessionOrder {
            project: project.id,
            expected: vec![third, second, first],
            sessions: vec![first, third, second],
        });
        client.execute(reorder.clone()).await.unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[first, third, second],
        )
        .await;
        client.execute(reorder).await.unwrap();
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[first, third, second],
        )
        .await;
        let conflict = client
            .execute(client.prepare(Command::SetSessionOrder {
                project: project.id,
                expected: vec![third, second, first],
                sessions: vec![second, third, first],
            }))
            .await
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::RevisionConflict);
        for sessions in [
            vec![first, first, second],
            vec![first, third, SessionId::new()],
        ] {
            let error = client
                .execute(client.prepare(Command::SetSessionOrder {
                    project: project.id,
                    expected: vec![first, third, second],
                    sessions,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
        }
        let Output::Session(newest) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(sessions[0].config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        ordered(
            &client,
            &mut *updates,
            &mut projection,
            &[newest.id, first, third, second],
        )
        .await;

        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
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
        let restored = snapshot(&client).await;
        assert_eq!(restored.sessions, projection.snapshot().unwrap().sessions);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
