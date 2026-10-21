use super::*;
use crate::store::database::Database;
use sailry_protocol::{ErrorCode, RequestId, RequestOutcome};

fn payload() -> Value {
    json!({
        "content":[{"type":"image","mimeType":"image/png","data":"AQID"}],
        "isError":false,"structuredContent":{"window":"original","nested":{"kind":"computer"}},
        "_meta":{"source":"retired-driver"}
    })
}

fn insert(db: &Connection, caller: NodeId, id: RequestId) -> (String, String, String) {
    let digest = blake3::hash(b"original use_computer command")
        .to_hex()
        .to_string();
    let body = format!("command-v1:{digest}");
    let result = json!({"Ok":{"kind":"computer","data":payload()}}).to_string();
    db.execute(
        "INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
        params![caller.0.as_slice(), id.to_string(), body.as_bytes(), result],
    )
    .unwrap();
    (digest, body, result)
}

fn snapshot(path: &std::path::Path) -> Connection {
    let mut uri = url::Url::from_file_path(path).unwrap();
    uri.query_pairs_mut()
        .append_pair("mode", "ro")
        .append_pair("immutable", "1");
    Connection::open_with_flags(
        uri.as_str(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap()
}

#[test]
fn retains_only_matching_output_payloads() {
    let fixture = Fixture::new();
    let original = payload();
    fixture.insert(
        "requests",
        &bytes(&json!({"Ok":{"kind":"computer","data":original}})),
    );
    assert!(reconcile(&fixture.db).unwrap());
    let after = fixture.body("requests");
    let output: Result<Output, sailry_protocol::Fault> = serde_json::from_slice(&after).unwrap();
    let Output::RetainedRecord(record) = output.unwrap() else {
        panic!("retained receipt expected")
    };
    assert_eq!(record.kind, "computer");
    assert_eq!(record.data, original);
    assert!(!reconcile(&fixture.db).unwrap());
    assert_eq!(fixture.body("requests"), after);

    for value in [
        json!({"Ok":{"kind":"plugin_result","data":{"kind":"computer","data":original}}}),
        json!({"Err":{"code":"unavailable","message":"computer"}}),
    ] {
        let fixture = Fixture::new();
        let before = bytes(&value);
        fixture.insert("requests", &before);
        assert!(!reconcile(&fixture.db).unwrap());
        assert_eq!(fixture.body("requests"), before);
    }
}

#[test]
fn backs_up_receipts_before_validation() {
    for invalid in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let node = NodeId([27; 32]);
        Database::open(&path, node, None).unwrap().close().unwrap();
        let db = Connection::open(&path).unwrap();
        let id = RequestId::new();
        let (_, body, before) = insert(&db, node, id);
        assert!(needed(&db).unwrap());
        if invalid {
            db.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",
                params![node.0.as_slice(), RequestId::new().to_string(), b"original-invalid".as_slice(), "{private-invalid-marker"]).unwrap();
        }
        drop(db);
        if invalid {
            let error = Database::open(&path, node, None)
                .err()
                .expect("invalid preview must fail")
                .to_string();
            assert!(!error.contains("private-invalid-marker"));
            assert!(!directory.path().join("backups").exists());
            let db = Connection::open(&path).unwrap();
            let unchanged: String = db
                .query_row(
                    "SELECT result FROM requests WHERE id=?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(unchanged, before);
        } else {
            let database = Database::open(&path, node, None).unwrap();
            assert!(!needed(&database.connection).unwrap());
            let (stored, status): (Vec<u8>, String) = database
                .connection
                .query_row(
                    "SELECT body,status FROM requests WHERE id=?1",
                    [id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(stored, body.as_bytes());
            assert_eq!(status, "completed");
            crate::store::database::validate_backup(&database.connection, node).unwrap();
            database.close().unwrap();
            let backups: Vec<_> = std::fs::read_dir(directory.path().join("backups"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(backups.len(), 1);
            let backup = snapshot(&backups[0]);
            let saved: String = backup
                .query_row(
                    "SELECT result FROM requests WHERE id=?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(saved, before);
            Database::open(&path, node, None).unwrap().close().unwrap();
            assert_eq!(
                std::fs::read_dir(directory.path().join("backups"))
                    .unwrap()
                    .count(),
                1
            );
        }
    }
}

#[tokio::test]
async fn observes_original_admissions_without_replay() {
    use sailry_client::Client;
    use sailry_link::{Link, NetworkScope};
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = crate::Node::start_empty(&profile).await.unwrap();
        let node_id = node.id();
        node.shutdown().await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let caller = if remote {
            NodeId(*controller.handle().address().id.as_bytes())
        } else {
            node_id
        };
        let id = RequestId::new();
        let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let (digest, body, before) = insert(&db, caller, id);
        drop(db);
        let node = crate::Node::start_empty(&profile).await.unwrap();
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
        for _ in 0..2 {
            let request = client.prepare(Command::InspectRequest {
                id,
                digest: digest.clone(),
            });
            assert!(!request.command.durable());
            let Output::RequestOutcome {
                outcome: RequestOutcome::Completed(result),
                ..
            } = client.execute(request).await.unwrap()
            else {
                panic!("completed admission expected")
            };
            let Output::RetainedRecord(record) = result.unwrap() else {
                panic!("retained computer receipt expected")
            };
            assert_eq!(record.kind, "computer");
            assert_eq!(record.data, payload());
        }
        let error = client
            .execute(client.prepare(Command::InspectRequest {
                id,
                digest: "0".repeat(64),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let db = Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let (stored, status, count): (Vec<u8>, String, i64) = db
            .query_row(
                "SELECT body,status,(SELECT count(*) FROM requests) FROM requests WHERE id=?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(stored, body.as_bytes());
        assert_eq!(status, "completed");
        assert_eq!(count, 1);
        crate::store::database::validate_backup(&db, node_id).unwrap();
        let backups: Vec<_> = std::fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = snapshot(&backups[0]);
        let saved: String = backup
            .query_row(
                "SELECT result FROM requests WHERE id=?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, before);
    }
}
