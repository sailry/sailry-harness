use std::time::Duration;

use sailry_client::{Apply, Client, Projection};
use sailry_protocol::*;
use tempfile::TempDir;

use crate::Node;

async fn node() -> (TempDir, Node, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("profile")).await.unwrap();
    let client = Client::new(node.local());
    (directory, node, client)
}

fn config(model: &str) -> SessionConfig {
    SessionConfig {
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: model.into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: None,
    }
}

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

async fn project(client: &Client, directory: &TempDir) -> Project {
    let request = client.prepare(Command::RegisterProject {
        name: "Project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let Output::Project(project) = client.execute(request).await.unwrap() else {
        panic!("project expected")
    };
    project
}

#[tokio::test]
async fn restart_preserves_durable_state() {
    let (directory, node, client) = node().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let original = client.execute(request.clone()).await.unwrap();
    assert_eq!(client.execute(request.clone()).await.unwrap(), original);
    let id = node.id();
    let path = node.profile().to_owned();
    node.shutdown().await.unwrap();
    let node = Node::start(path).await.unwrap();
    assert_eq!(node.id(), id);
    let client = Client::new(node.local());
    assert_eq!(client.execute(request).await.unwrap(), original);
    assert_eq!(snapshot(&client).await.projects.len(), 1);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_request_id_conflict() {
    let (directory, node, client) = node().await;
    let mut request = client.prepare(Command::RegisterProject {
        name: "First".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    client.execute(request.clone()).await.unwrap();
    request.command = Command::RegisterProject {
        name: "Changed".into(),
        path: directory.path().to_str().unwrap().into(),
    };
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(snapshot(&client).await.projects[0].name, "First");
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn detaching_preserves_committed_work() {
    let (directory, node, client) = node().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let connection =
        rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let rows: i64 = connection
        .query_row(
            "SELECT count(*) FROM requests WHERE id=?1",
            [request.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "a durable receipt requires committed admission");
    drop(admission);
    assert!(matches!(
        client.execute(request).await.unwrap(),
        Output::Project(_)
    ));
    assert_eq!(snapshot(&client).await.projects.len(), 1);
    drop(connection);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn freezes_session_and_queue_config() {
    let (directory, node, client) = node().await;
    let project = project(&client, &directory).await;
    let original = config("original");
    client
        .execute(client.prepare(Command::SetDefaults {
            expected_revision: 0,
            config: original.clone(),
        }))
        .await
        .unwrap();
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            worktree: None,
            project: Some(project.id),
            config: None,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let turn = client.prepare(Command::QueueTurn {
        session: session.id,
        expected_revision: 1,
        message: "Persist admission, do not execute an Agent".into(),
    });
    client.execute(turn.clone()).await.unwrap();
    let changed = config("changed");
    client
        .execute(client.prepare(Command::SetDefaults {
            expected_revision: 1,
            config: changed.clone(),
        }))
        .await
        .unwrap();
    let second = Client::new(node.local());
    assert_eq!(snapshot(&second).await.sessions[0].config, original);
    client
        .execute(client.prepare(Command::SetSessionConfig {
            session: session.id,
            expected_revision: 1,
            config: changed.clone(),
        }))
        .await
        .unwrap();
    let conflict = client.prepare(Command::SetSessionConfig {
        session: session.id,
        expected_revision: 1,
        config: original.clone(),
    });
    assert_eq!(
        client.execute(conflict).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let Output::QueuedTurn(replayed) = client.execute(turn).await.unwrap() else {
        panic!("queued turn expected")
    };
    assert_eq!(replayed.revision, 1);
    assert_eq!(replayed.config, original);
    let state = snapshot(&second).await;
    assert_eq!(state.sessions[0].config, changed);
    assert_eq!(state.turns[0].config, original);
    assert_eq!(state.turns.len(), 1);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_foreign_credentials() {
    let (_directory, node, client) = node().await;
    let mut config = config("model");
    config.credential = Some(CredentialRef {
        node: NodeId([7; 32]),
        id: CredentialId::new(),
    });
    let request = client.prepare(Command::SetDefaults {
        expected_revision: 0,
        config,
    });
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(snapshot(&client).await.defaults.revision, 0);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn publishes_commits_until_shutdown() {
    let (directory, node, client) = node().await;
    let mut subscription = client.subscribe().await.unwrap();
    let mut projection = Projection::new(node.id(), 1);
    assert_eq!(
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap(),
        Apply::Applied
    );
    project(&client, &directory).await;
    assert_eq!(
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap(),
        Apply::Recover
    );
    client.recover(&mut projection, 1).await.unwrap();
    assert_eq!(projection.snapshot().unwrap(), &snapshot(&client).await);
    let waiting = tokio::spawn(async move { subscription.next().await });
    node.shutdown().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn recovers_lagged_subscriber() {
    let (_directory, node, client) = node().await;
    let mut subscription = client.subscribe().await.unwrap();
    let mut projection = Projection::new(node.id(), 1);
    projection
        .apply(1, subscription.next().await.unwrap())
        .unwrap();
    for revision in 0..70 {
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: revision,
                config: config("model"),
            }))
            .await
            .unwrap();
    }
    assert_eq!(subscription.next().await.unwrap(), Update::ResetRequired);
    client.recover(&mut projection, 1).await.unwrap();
    assert_eq!(projection.snapshot().unwrap().defaults.revision, 70);
    assert_eq!(
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap(),
        Apply::Ignored
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_interrupted_admission() {
    let (directory, node, client) = node().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Uncertain".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let path = node.profile().to_owned();
    let caller = node.id();
    node.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(path.join("storage/node.sqlite3")).unwrap();
    connection
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            rusqlite::params![
                &caller.0[..],
                request.id.to_string(),
                serde_json::to_vec(&request).unwrap()
            ],
        )
        .unwrap();
    drop(connection);
    let node = Node::start(path).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(snapshot(&client).await.projects.is_empty());
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupted_command_preserves_uncertainty() {
    let (_directory, node, client) = node().await;
    let request = client.prepare(Command::RunCommand {
        turn: TurnId::new(),
        command: "exit 0".into(),
        cwd: String::new(),
        timeout_ms: 1_000,
        background: false,
        attachments: Vec::new(),
    });
    let path = node.profile().to_owned();
    let caller = node.id();
    node.shutdown().await.unwrap();
    let connection = rusqlite::Connection::open(path.join("storage/node.sqlite3")).unwrap();
    let body = format!(
        "command-v1:{}",
        blake3::hash(&serde_json::to_vec(&request).unwrap()).to_hex()
    );
    connection
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            rusqlite::params![&caller.0[..], request.id.to_string(), body.as_bytes()],
        )
        .unwrap();
    drop(connection);
    let node = Node::start(path).await.unwrap();
    let client = Client::new(node.local());
    for _ in 0..2 {
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Unknown
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
    }
    let mut changed = request;
    let Command::RunCommand { command, .. } = &mut changed.command else {
        unreachable!()
    };
    *command = "exit 1".into();
    assert_eq!(
        client.outcome(&changed).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        client.execute(changed).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    node.shutdown().await.unwrap();
}
