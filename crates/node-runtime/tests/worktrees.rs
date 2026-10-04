use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[path = "worktrees/conversations.rs"]
mod conversations;
#[path = "worktrees/creation.rs"]
mod creation;
#[path = "worktrees/discovery.rs"]
mod discovery;
#[path = "worktrees/recovery.rs"]
mod recovery;
#[path = "worktrees/removal.rs"]
mod removal;

fn config() -> SessionConfig {
    SessionConfig {
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: "test".into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: None,
    }
}
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
async fn register(client: &Client, path: &std::path::Path) -> Project {
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Project".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    project
}

async fn ownership(remote: bool) {
    let directory = tempfile::tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let execution = Node::start(directory.path().join("execution"))
        .await
        .unwrap();
    controller
        .link()
        .set_trust(execution.id(), true)
        .await
        .unwrap();
    execution
        .link()
        .set_trust(controller.id(), true)
        .await
        .unwrap();
    let client = Client::new(if remote {
        controller.link().remote(execution.link().address())
    } else {
        execution.local()
    });
    let first_path = directory.path().join("first");
    let second_path = directory.path().join("second");
    std::fs::create_dir(&first_path).unwrap();
    std::fs::create_dir(&second_path).unwrap();
    let first = register(&client, &first_path).await;
    let second = register(&client, &second_path).await;
    let state = snapshot(&client).await;
    assert_eq!(state.worktrees.len(), 2);
    let main = state
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(first.id))
        .unwrap();
    assert!(main.main);
    assert_eq!(main.path, first.path);
    let other = state
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(second.id))
        .unwrap();
    let request = client.prepare(Command::CreateSession {
        project: Some(first.id),
        worktree: Some(other.id),
        config: Some(config()),
    });
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::NotFound
    );
    for selected in [None, Some(main.id)] {
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(first.id),
                worktree: selected,
                config: Some(config()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(session.worktree, main.id);
        assert_eq!(session.project, Some(first.id));
    }
    let path = execution.profile().to_owned();
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    let execution = Node::start(path).await.unwrap();
    let resumed = snapshot(&Client::new(execution.local())).await;
    assert_eq!(resumed.worktrees, state.worktrees);
    assert!(
        resumed
            .sessions
            .iter()
            .all(|session| session.worktree == main.id)
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_ownership_survives_restart() {
    ownership(false).await;
}

#[tokio::test]
async fn remote_ownership_survives_restart() {
    ownership(true).await;
}

#[tokio::test]
async fn restores_sessions_and_receipts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node");
    let node = Node::start(&path).await.unwrap();
    let client = Client::new(node.local());
    let project = register(&client, directory.path()).await;
    let request = client.prepare(Command::CreateSession {
        project: Some(project.id),
        worktree: None,
        config: Some(config()),
    });
    let Output::Session(original) = client.execute(request.clone()).await.unwrap() else {
        panic!("session expected")
    };
    node.shutdown().await.unwrap();
    let node = Node::start(path).await.unwrap();
    let client = Client::new(node.local());
    let state = snapshot(&client).await;
    assert_eq!(state.worktrees.len(), 1);
    let Output::Session(replayed) = client.execute(request).await.unwrap() else {
        panic!("session expected")
    };
    assert_eq!(replayed.id, original.id);
    assert_eq!(replayed.config, original.config);
    assert_eq!(replayed.worktree, state.worktrees[0].id);
    assert_eq!(state.sessions, vec![replayed]);
    node.shutdown().await.unwrap();
}
