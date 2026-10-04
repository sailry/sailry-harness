use sailry_client::{Client, Projection};
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[path = "sessions/attention.rs"]
mod attention;

#[path = "sessions/order.rs"]
mod order;

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(value) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    value
}

#[tokio::test]
async fn archives_restores_and_removes() {
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
                name: "Lifecycle fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let original = snapshot(&client).await;
        let archive = client.prepare(Command::SetSessionArchived {
            session: session.id,
            expected_revision: 1,
            archived: true,
        });
        let Output::Session(archived) = client.execute(archive.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert!(archived.archived);
        assert_eq!(archived.revision, 2);
        assert_eq!(
            client.execute(archive).await.unwrap(),
            Output::Session(archived.clone())
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionArchived {
                    session: session.id,
                    expected_revision: 1,
                    archived: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Session(restored) = client
            .execute(client.prepare(Command::SetSessionArchived {
                session: session.id,
                expected_revision: 2,
                archived: false,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert!(!restored.archived);
        assert_eq!(snapshot(&client).await.sessions, vec![restored.clone()]);
        let delete = client.prepare(Command::RemoveSession {
            session: session.id,
            expected_revision: 3,
        });
        let result = client.execute(delete.clone()).await.unwrap();
        assert_eq!(result, Output::SessionsRemoved(vec![session.id]));
        assert_eq!(client.execute(delete).await.unwrap(), result);
        assert!(snapshot(&client).await.sessions.is_empty());
        assert_eq!(
            client
                .execute(client.prepare(Command::SetSessionArchived {
                    session: session.id,
                    expected_revision: 3,
                    archived: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut projection = Projection::new(node.id(), 1);
        let cursor = original.cursor;
        projection.apply(1, Update::Snapshot(original)).unwrap();
        for (index, event) in [
            Event::SessionChanged(Box::new(archived)),
            Event::SessionChanged(Box::new(restored)),
            Event::SessionsRemoved(vec![session.id]),
        ]
        .into_iter()
        .enumerate()
        {
            projection
                .apply(
                    1,
                    Update::Event(EventEnvelope {
                        node: node.id(),
                        cursor: cursor + index as u64 + 1,
                        event,
                    }),
                )
                .unwrap();
        }
        assert!(projection.snapshot().unwrap().sessions.is_empty());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        assert!(
            snapshot(&Client::new(node.local()))
                .await
                .sessions
                .is_empty()
        );
        node.shutdown().await.unwrap();
    }
}
