use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[path = "credentials/key.rs"]
mod key;
#[path = "credentials/ownership.rs"]
mod ownership;

async fn lifecycle(remote: bool) {
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
    let id = CredentialId::new();
    let provider = ProviderId::new();
    let reference = CredentialRef {
        node: execution.id(),
        id,
    };
    let marker = "isolated-test-credential-never-in-projections";
    let request = client.prepare(Command::PutCredential {
        id,
        provider,
        expected_revision: 0,
        secret: Secret::new(marker.into()),
        expires_at_ms: None,
    });
    assert!(!format!("{request:?}").contains(marker));
    let first = client.execute(request.clone()).await.unwrap();
    assert_eq!(client.execute(request.clone()).await.unwrap(), first);
    let mut conflict = request.clone();
    if let Command::PutCredential { secret, .. } = &mut conflict.command {
        *secret = Secret::new("different".into());
    }
    assert_eq!(
        client.execute(conflict).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        execution
            .resolve_credential(reference.clone(), provider)
            .await
            .unwrap()
            .expose(),
        marker
    );
    assert_eq!(
        controller
            .resolve_credential(reference.clone(), provider)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        execution
            .resolve_credential(reference.clone(), ProviderId::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );

    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider,
        model: "configured".into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: Some(reference.clone()),
    };
    client
        .execute(client.prepare(Command::SetDefaults {
            expected_revision: 0,
            config: config.clone(),
        }))
        .await
        .unwrap();
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Project".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
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
    let queue = || Command::QueueTurn {
        session: session.id,
        expected_revision: 1,
        message: "admission only".into(),
    };
    client.execute(client.prepare(queue())).await.unwrap();
    client
        .execute(client.prepare(Command::RevokeCredential {
            id,
            expected_revision: 1,
        }))
        .await
        .unwrap();
    assert_eq!(
        client
            .execute(client.prepare(queue()))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        execution
            .resolve_credential(reference.clone(), provider)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    client
        .execute(client.prepare(Command::PutCredential {
            id,
            provider,
            expected_revision: 2,
            secret: Secret::new(marker.into()),
            expires_at_ms: None,
        }))
        .await
        .unwrap();
    client.execute(client.prepare(queue())).await.unwrap();
    for command in [Command::Snapshot, Command::ListCredentials] {
        let result = client.execute(client.prepare(command)).await.unwrap();
        assert!(!serde_json::to_string(&result).unwrap().contains(marker));
    }
    let db = rusqlite::Connection::open(execution.profile().join("storage/node.sqlite3")).unwrap();
    for query in [
        "SELECT CAST(body AS TEXT) FROM requests",
        "SELECT result FROM requests",
        "SELECT CAST(body AS TEXT) FROM events",
    ] {
        let mut statement = db.prepare(query).unwrap();
        for row in statement
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .unwrap()
        {
            assert!(!row.unwrap().unwrap_or_default().contains(marker));
        }
    }
    drop(db);
    let path = execution.profile().to_owned();
    controller.shutdown().await.unwrap();
    execution.shutdown().await.unwrap();
    let restored = Node::start(path).await.unwrap();
    assert_eq!(
        restored
            .resolve_credential(reference, provider)
            .await
            .unwrap()
            .expose(),
        marker
    );
    let resumed = Client::new(restored.local());
    resumed.execute(resumed.prepare(queue())).await.unwrap();
    restored.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_lifecycle_and_redaction() {
    lifecycle(false).await;
}

#[tokio::test]
async fn remote_lifecycle_and_redaction() {
    lifecycle(true).await;
}

#[tokio::test]
async fn rejects_unavailable_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let provider = ProviderId::new();
    let id = CredentialId::new();
    let reference = CredentialRef {
        node: node.id(),
        id,
    };
    let config = SessionConfig {
        assistant: None,
        resource: None,
        provider,
        model: "configured".into(),
        effort: Effort::High,
        mode: sailry_protocol::WorkMode::Code,
        permission: sailry_protocol::Permission::Ask,
        credential: Some(reference.clone()),
    };
    assert_eq!(
        client
            .execute(client.prepare(Command::SetDefaults {
                expected_revision: 0,
                config
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        node.resolve_credential(reference.clone(), provider)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let put = |revision, expiry| Command::PutCredential {
        id,
        provider,
        expected_revision: revision,
        secret: Secret::new("test-only".into()),
        expires_at_ms: expiry,
    };
    assert_eq!(
        client
            .execute(client.prepare(put(0, Some(1))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        client
            .execute(client.prepare(put(0, Some(u64::MAX))))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    client.execute(client.prepare(put(0, None))).await.unwrap();
    assert_eq!(
        client
            .execute(client.prepare(put(0, None)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    // Model elapsed wall time without slowing the test or modifying the process clock.
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    db.execute(
        "UPDATE providers SET body=CAST(json_set(body,?1,1) AS BLOB) WHERE id=?2",
        rusqlite::params![
            format!("$.authorizations.\"{id}\".expires_at_ms"),
            provider.to_string()
        ],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        node.resolve_credential(reference, provider)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_unsupported_storage() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node");
    let node = Node::start(&path).await.unwrap();
    node.shutdown().await.unwrap();
    let db = rusqlite::Connection::open(path.join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    db.pragma_update(None, "user_version", 2).unwrap();
    drop(db);
    let database = path.join("storage/node.sqlite3");
    let before = std::fs::read(&database).unwrap();
    assert!(Node::start(&path).await.is_err());
    assert_eq!(std::fs::read(database).unwrap(), before);
}
