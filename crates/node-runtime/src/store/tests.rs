use std::time::Duration;

use sailry_client::{Apply, Client, Projection};
use sailry_protocol::*;
use tempfile::TempDir;

use crate::Node;

mod concurrency;

async fn node() -> (TempDir, Node, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start_empty(directory.path().join("profile"))
        .await
        .unwrap();
    let client = Client::new(node.local());
    (directory, node, client)
}

pub(super) fn config(model: &str) -> SessionConfig {
    SessionConfig {
        features: Default::default(),
        assistant: None,
        resource: None,
        provider: ProviderId::new(),
        model: model.into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        options: Default::default(),
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
async fn detaching_preserves_committed_work() {
    let (directory, node, client) = node().await;
    let request = client.prepare(Command::RegisterProject {
        name: "Project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let admission = client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    let id = request.id;
    let rows: i64 = node
        .with_storage(move |connection| {
            Box::pin(async move {
                connection
                    .prepare("SELECT count(*) FROM requests WHERE id=?1")
                    .await
                    .map_err(super::database::storage_error)?
                    .query_row([id.to_string()])
                    .await
                    .map_err(super::database::storage_error)?
                    .get(0)
                    .map_err(super::database::storage_error)
            })
        })
        .await
        .unwrap();
    assert_eq!(rows, 1, "a durable receipt requires committed admission");
    drop(admission);
    assert!(matches!(
        client.execute(request).await.unwrap(),
        Output::Project(_)
    ));
    assert_eq!(snapshot(&client).await.projects.len(), 1);
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
    let mut changed = config("changed");
    changed.effort = Effort::High;
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
    assert_eq!(
        client
            .read_turn_capture(session.id, state.turns[0].id)
            .await
            .unwrap()
            .config,
        original
    );
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
    for remote in [false, true] {
        let (directory, node, local) = node().await;
        let controller = sailry_link::Link::controller(
            directory.path().join("controller"),
            sailry_link::NetworkScope::default(),
        )
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
        let mut subscription = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap();
        let initial = projection.snapshot().unwrap().cursor;
        let revisions = (super::CAPACITY + 1) as u64;
        for revision in 0..revisions {
            client
                .execute(client.prepare(Command::SetDefaults {
                    expected_revision: revision,
                    config: config("model"),
                }))
                .await
                .unwrap();
        }
        let mut last = None;
        for revision in 1..=revisions {
            let update = tokio::time::timeout(Duration::from_secs(5), subscription.next())
                .await
                .unwrap()
                .unwrap();
            let Update::Event(event) = &update else {
                panic!("durable event expected");
            };
            assert_eq!(event.cursor, initial + revision);
            assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
            assert_eq!(projection.snapshot().unwrap().defaults.revision, revision);
            last = Some(update);
        }
        client.recover(&mut projection, 1).await.unwrap();
        assert_eq!(projection.snapshot().unwrap(), &snapshot(&client).await);
        assert_eq!(projection.snapshot().unwrap().defaults.revision, revisions);
        assert_eq!(projection.apply(1, last.unwrap()).unwrap(), Apply::Ignored);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
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
    let database = super::database::engine(&path.join("storage/node.turso"))
        .await
        .unwrap();
    let mut connection = super::database::connect(&database).await.unwrap();
    let transaction = connection.transaction().await.unwrap();
    transaction
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            turso::params![
                &caller.0[..],
                request.id.to_string(),
                serde_json::to_vec(&request).unwrap()
            ],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    drop(connection);
    drop(database);
    let node = Node::start_empty(path).await.unwrap();
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
        access: sailry_protocol::process::Access::Unrestricted,
        description: None,
        cwd: String::new(),
        timeout_ms: 1_000,
        pty: false,
        background: false,
        attachments: Vec::new(),
    });
    let path = node.profile().to_owned();
    let caller = node.id();
    node.shutdown().await.unwrap();
    let database = super::database::engine(&path.join("storage/node.turso"))
        .await
        .unwrap();
    let mut connection = super::database::connect(&database).await.unwrap();
    let transaction = connection.transaction().await.unwrap();
    let body = format!(
        "command-v1:{}",
        blake3::hash(&serde_json::to_vec(&request).unwrap()).to_hex()
    );
    transaction
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            turso::params![&caller.0[..], request.id.to_string(), body.as_bytes()],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    drop(connection);
    drop(database);
    let node = Node::start_empty(path).await.unwrap();
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
