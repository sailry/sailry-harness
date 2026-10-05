use crate::file_fixture::Fixture;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::*;

fn counts(db: &rusqlite::Connection) -> (i64, i64) {
    db.query_row(
        "SELECT (SELECT count(*) FROM requests), (SELECT count(*) FROM events)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[tokio::test]
async fn observes_existing_requests() {
    let fixture = Fixture::start().await;
    let db =
        rusqlite::Connection::open(fixture._temp.path().join("node/storage/node.sqlite3")).unwrap();
    let clients = fixture.clients();
    for (index, client) in clients.iter().enumerate() {
        let path = format!("directory-{index}");
        let request = client.prepare(Command::CreateDirectory {
            worktree: fixture.worktree,
            path: path.clone(),
        });
        let before = counts(&db);
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert!(!fixture.root.join(&path).exists());
        assert_eq!(counts(&db), before);
        let result = client.execute(request.clone()).await;
        assert!(result.is_ok());
        std::fs::remove_dir(fixture.root.join(&path)).unwrap();
        let before = counts(&db);
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(result)),
        );
        assert!(!fixture.root.join(&path).exists());
        // Even a trusted peer cannot observe another caller's admission.
        assert_eq!(
            clients[1 - index].outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted,
        );
        let mut changed = request.clone();
        changed.command = Command::CreateDirectory {
            worktree: fixture.worktree,
            path: "different".into(),
        };
        assert_eq!(
            client.outcome(&changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        for digest in ["", "invalid", &"A".repeat(64)] {
            let query = client.prepare(Command::InspectRequest {
                id: request.id,
                digest: digest.into(),
            });
            assert!(!query.command.durable());
            assert_eq!(
                client.execute(query).await.unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
        let mut wrong = request.clone();
        wrong.target = NodeId([9; 32]);
        assert_eq!(
            client.outcome(&wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        wrong = request.clone();
        wrong.version += 1;
        assert_eq!(
            client.outcome(&wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            client
                .outcome(&client.prepare(Command::Snapshot))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest,
        );
        assert_eq!(counts(&db), before);

        let failed = client.prepare(Command::CreateDirectory {
            worktree: WorktreeId::new(),
            path: "missing".into(),
        });
        let result = client.execute(failed.clone()).await;
        assert_eq!(result.as_ref().unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(
            client.outcome(&failed).await.unwrap(),
            RequestOutcome::Completed(Box::new(result)),
        );
    }
    drop(db);
    fixture.close().await;
}

#[tokio::test]
async fn matches_private_content() {
    let fixture = Fixture::start().await;
    let db =
        rusqlite::Connection::open(fixture._temp.path().join("node/storage/node.sqlite3")).unwrap();
    let marker = "isolated-outcome-test-private-content";
    for (index, client) in fixture.clients().iter().enumerate() {
        let commands = [
            Command::WriteFile {
                worktree: fixture.worktree,
                path: format!("file-{index}"),
                text: marker.into(),
                expected_revision: None,
            },
            Command::PutCredential {
                id: CredentialId::new(),
                provider: ProviderId::new(),
                expected_revision: 0,
                secret: Secret::new(marker.into()),
                expires_at_ms: None,
            },
        ];
        for command in commands {
            let mut request = client.prepare(command);
            let result = client.execute(request.clone()).await;
            assert!(result.is_ok());
            let before = counts(&db);
            let outcome = client.outcome(&request).await.unwrap();
            assert_eq!(outcome, RequestOutcome::Completed(Box::new(result)));
            assert!(!serde_json::to_string(&outcome).unwrap().contains(marker));
            let body: Vec<u8> = db
                .query_row(
                    "SELECT body FROM requests WHERE id=?1",
                    [request.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            let body = String::from_utf8(body).unwrap();
            assert!(body.starts_with("file-v1:") || body.starts_with("credential-v1:"));
            assert!(!body.contains(marker));
            match &mut request.command {
                Command::WriteFile { text, .. } => *text = "different".into(),
                Command::PutCredential { secret, .. } => *secret = Secret::new("different".into()),
                _ => unreachable!(),
            }
            assert_eq!(
                client.outcome(&request).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(counts(&db), before);
        }
    }
    drop(db);
    fixture.close().await;
}

#[tokio::test]
async fn survives_result_loss_and_restart() {
    let fixture = Fixture::start().await;
    let db =
        rusqlite::Connection::open(fixture._temp.path().join("node/storage/node.sqlite3")).unwrap();
    let mut pending = Vec::new();
    for (index, client) in fixture.clients().iter().enumerate() {
        let completed = client.prepare(Command::CreateDirectory {
            worktree: fixture.worktree,
            path: format!("completed-{index}"),
        });
        let result = client.execute(completed.clone()).await.unwrap();
        db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let path = format!("uncertain-{index}");
        let request = client.prepare(Command::WriteFile {
            worktree: fixture.worktree,
            path: path.clone(),
            text: "original".into(),
            expected_revision: None,
        });
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(fixture.root.join(&path)).unwrap(),
            b"original"
        );
        // Result loss leaves a durable admission, not a claim of completion.
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Admitted
        );
        std::fs::write(fixture.root.join(&path), "external replacement").unwrap();
        db.execute_batch("DROP TRIGGER lose_result").unwrap();
        pending.push((completed, result, request, path));
    }
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(fixture._temp.path().join("node"))
        .await
        .unwrap();
    let clients = [
        Client::new(node.local()),
        Client::new(fixture.controller.handle().remote(node.link().address())),
    ];
    let before = counts(&db);
    for (client, (completed, result, request, path)) in clients.iter().zip(pending) {
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Unknown
        );
        assert_eq!(
            client.outcome(&completed).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(result))),
        );
        assert_eq!(
            std::fs::read(fixture.root.join(path)).unwrap(),
            b"external replacement"
        );
    }
    assert_eq!(counts(&db), before);
    drop(db);
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
