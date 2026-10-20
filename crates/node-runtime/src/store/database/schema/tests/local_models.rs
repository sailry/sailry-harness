use super::*;
use crate::{
    models::{registry::Registry, test_support::record},
    store::{database::encode, local_models},
};
use rusqlite::params;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Event, KnowledgeId, ModelId, Output, RequestId,
    models::{Kind, State},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Fixture {
    directory: tempfile::TempDir,
    path: PathBuf,
    node: NodeId,
    model: String,
    ocr: String,
    definition: sailry_protocol::models::Definition,
    collection: KnowledgeId,
    raw: Vec<u8>,
    completed: RequestId,
    pending: RequestId,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let path = root.join("node.sqlite3");
        let node = NodeId([71; 32]);
        let db = Database::open(&path, node, None).unwrap();
        db.connection.execute_batch("DROP TABLE local_models; DROP TABLE local_model_settings;
            CREATE TABLE knowledge_models(id TEXT PRIMARY KEY,digest TEXT NOT NULL UNIQUE,body BLOB NOT NULL);
            CREATE TABLE knowledge_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);
            CREATE TABLE ocr_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);").unwrap();
        let (registration, contents) = record(2, &[100., 0., 1., 0., 0., 1.]);
        let definition = registration.definition;
        let id = ModelId::new();
        let model = format!("model-{id}");
        let body = encode(&definition).unwrap();
        db.connection
            .execute(
                "INSERT INTO knowledge_models(id,digest,body) VALUES(?1,?2,?3)",
                params![
                    id.to_string(),
                    blake3::hash(&body).to_hex().to_string(),
                    body
                ],
            )
            .unwrap();
        db.connection.execute("INSERT INTO knowledge_settings(singleton,body) VALUES(1,?1)",[encode(&json!({"revision":19,"model":{"registered":{"id":id}},"mirror":"https://models.example.invalid/cache"})).unwrap()]).unwrap();
        let mut ocr_definition = crate::models::registry::catalog::ocr()[0].clone();
        ocr_definition.name = "Imported OCR fixture".into();
        let ocr_contents = ocr_definition
            .assets
            .iter_mut()
            .map(|asset| {
                let body = if asset.name == "dictionary.txt" {
                    b"a\n".to_vec()
                } else {
                    b"receipt graph fixture".to_vec()
                };
                asset.bytes = body.len() as u64;
                asset.sha256 = format!("{:x}", Sha256::digest(&body));
                (asset.name.clone(), body)
            })
            .collect::<Vec<_>>();
        let ocr = crate::models::registry::catalog::id(&ocr_definition);
        db.connection
            .execute(
                "INSERT INTO ocr_settings(singleton,body) VALUES(1,?1)",
                [encode(
                    &json!({"settings":{"revision":7,"enabled":false,"model":ocr},"records":[ocr_definition]}),
                )
                .unwrap()],
            )
            .unwrap();
        let assets = root.join("models/embedding").join(&model).join("files");
        crate::profile::private_directory(&assets).unwrap();
        for (name, body) in contents {
            std::fs::write(assets.join(name), body).unwrap();
        }
        let handle =
            cap_std::fs::Dir::open_ambient_dir(&assets, cap_std::ambient_authority()).unwrap();
        sailry_model_assets::verified::write_marker(
            &handle,
            &blake3::hash(&encode(&definition).unwrap())
                .to_hex()
                .to_string(),
        )
        .unwrap();
        let ocr_assets = root.join("models/ocr").join(&ocr);
        crate::profile::private_directory(&ocr_assets).unwrap();
        for (name, body) in ocr_contents {
            std::fs::write(ocr_assets.join(name), body).unwrap();
        }
        let handle =
            cap_std::fs::Dir::open_ambient_dir(&ocr_assets, cap_std::ambient_authority()).unwrap();
        sailry_model_assets::verified::write_marker(&handle, &ocr).unwrap();
        let collection = KnowledgeId::new();
        let mut stored = json!({
            "collection":{"id":collection,"revision":5,"rebuild":2,"owner":{"kind":"controller"},
                "definition":{"name":"Imported source","source":{"kind":"directory","path":root},"model":{"registered":{"id":id}},"globs":[],"tags":["kept"],"shared_with":[]},
                "paused":false,"progress":{"status":"ready","sources":1,"completed":1,"chunks":2,"skipped":0,"model":{"registered":{"id":id}},"generation":8}},
            "capture":{"package":null,"worktree":null,"session":null},"automatic":false
        });
        db.connection
            .execute(
                "INSERT INTO knowledge_collections(id,revision,generation,body) VALUES(?1,5,8,?2)",
                params![collection.to_string(), encode(&stored).unwrap()],
            )
            .unwrap();
        let raw = encode(&json!({"version":1,"target":node,"id":RequestId::new(),"command":{"kind":"save_knowledge_settings","data":{"revision":19,"model":{"registered":{"id":id}},"mirror":null}}})).unwrap();
        let completed = RequestId::new();
        let pending = RequestId::new();
        db.connection.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",params![node.0.as_slice(),completed.to_string(),raw,json!({"Ok":{"kind":"knowledge_settings","data":{"revision":19,"model":{"registered":{"id":id}},"mirror":null}}}).to_string()]).unwrap();
        db.connection
            .execute(
                "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                params![node.0.as_slice(), pending.to_string(), raw],
            )
            .unwrap();
        db.connection
            .execute(
                "INSERT INTO events(body) VALUES(?1)",
                [encode(&json!({"kind":"ocr_settings_changed","data":{"revision":7}})).unwrap()],
            )
            .unwrap();
        // Collection snapshots retain their typed current shape after the ID conversion.
        stored = stored["collection"].take();
        db.connection.execute("INSERT INTO requests(caller,id,body,status,result) VALUES(?1,?2,?3,'completed',?4)",params![node.0.as_slice(),RequestId::new().to_string(),b"retained snapshot".as_slice(),json!({"Ok":{"kind":"knowledge_collection","data":stored}}).to_string()]).unwrap();
        db.close().unwrap();
        Self {
            directory,
            path,
            node,
            model,
            ocr,
            definition,
            collection,
            raw,
            completed,
            pending,
        }
    }
}

#[tokio::test]
async fn preserves_installed_identity_and_imports_once() {
    let fixture = Fixture::new();
    let db = Database::open(&fixture.path, fixture.node, None).unwrap();
    let catalog = local_models::read(&db.connection).unwrap();
    assert_eq!(catalog.settings.revision, 0);
    assert_eq!(
        catalog.settings.selected(Kind::Embedding),
        Some(fixture.model.as_str())
    );
    assert_eq!(
        catalog.settings.mirror.as_deref(),
        Some("https://models.example.invalid/cache")
    );
    assert!(!catalog.settings.choice(Kind::Ocr).enabled);
    assert_eq!(
        catalog.settings.choice(Kind::Ocr).model.as_deref(),
        Some(fixture.ocr.as_str())
    );
    assert_eq!(catalog.records.len(), 2);
    let imported = catalog
        .records
        .iter()
        .find(|record| record.id == fixture.model)
        .unwrap();
    assert_eq!(imported.definition, fixture.definition);
    let stored = crate::store::knowledge::catalog(&db.connection)
        .unwrap()
        .collections
        .remove(0);
    assert_eq!(stored.collection.id, fixture.collection);
    assert_eq!(stored.collection.revision, 5);
    assert_eq!(stored.collection.rebuild, 2);
    assert_eq!(stored.collection.progress.generation, 8);
    assert_eq!(
        stored.collection.definition.model.as_deref(),
        Some(fixture.model.as_str())
    );
    assert_eq!(
        stored.collection.progress.model.as_deref(),
        Some(fixture.model.as_str())
    );
    let registry = Registry::new(
        Some(fixture.directory.path().into()),
        CancellationToken::new(),
    );
    registry
        .configure(catalog.settings, &catalog.records)
        .unwrap();
    let model = registry
        .models(Some(Kind::Embedding))
        .await
        .unwrap()
        .into_iter()
        .find(|model| model.id == fixture.model)
        .unwrap();
    assert_eq!(model.state, State::Installed);
    let ocr = registry
        .models(Some(Kind::Ocr))
        .await
        .unwrap()
        .into_iter()
        .find(|model| model.id == fixture.ocr)
        .unwrap();
    assert_eq!(ocr.state, State::Installed);
    registry.shutdown().await;
    let backups = std::fs::read_dir(fixture.directory.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let original =
        Connection::open_with_flags(&backups[0], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let old: Vec<u8> = original
        .query_row("SELECT body FROM knowledge_settings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&old).unwrap()["revision"],
        19
    );
    assert!(
        original
            .prepare("SELECT body FROM local_model_settings")
            .is_err()
    );
    // Emptying the new registry is an explicit current state, never an import trigger.
    db.connection
        .execute("DELETE FROM knowledge_collections", [])
        .unwrap();
    db.connection
        .execute("DELETE FROM local_models", [])
        .unwrap();
    let mut settings = local_models::settings(&db.connection).unwrap();
    settings.choices.insert(
        Kind::Embedding,
        sailry_protocol::models::Choice {
            enabled: false,
            model: None,
        },
    );
    settings.choices.insert(
        Kind::Ocr,
        sailry_protocol::models::Choice {
            enabled: false,
            model: None,
        },
    );
    db.connection
        .execute(
            "UPDATE local_model_settings SET body=?1",
            [encode(&settings).unwrap()],
        )
        .unwrap();
    db.close().unwrap();
    for _ in 0..2 {
        let db = Database::open(&fixture.path, fixture.node, None).unwrap();
        assert!(
            local_models::read(&db.connection)
                .unwrap()
                .records
                .is_empty()
        );
        assert_eq!(local_models::settings(&db.connection).unwrap(), settings);
        assert_eq!(
            db.connection
                .query_row("SELECT count(*) FROM knowledge_models", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        db.close().unwrap();
    }
}

#[test]
fn retains_receipts_events_and_pending_identity() {
    let fixture = Fixture::new();
    let db = Database::open(&fixture.path, fixture.node, None).unwrap();
    for request in [fixture.completed, fixture.pending] {
        let raw: Vec<u8> = db
            .connection
            .query_row(
                "SELECT body FROM requests WHERE id=?1",
                [request.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw, fixture.raw);
    }
    let completed: String = db
        .connection
        .query_row(
            "SELECT result FROM requests WHERE id=?1",
            [fixture.completed.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let result: Result<Output, sailry_protocol::Fault> = serde_json::from_str(&completed).unwrap();
    let Output::RetainedRecord(retained) = result.unwrap() else {
        panic!("retained output expected")
    };
    assert_eq!(retained.kind, "knowledge_settings");
    assert_eq!(retained.data["revision"], 19);
    let status: String = db
        .connection
        .query_row(
            "SELECT status FROM requests WHERE id=?1",
            [fixture.pending.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "unknown");
    let events = db
        .connection
        .prepare("SELECT body FROM events ORDER BY cursor")
        .unwrap()
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice::<Event>(&row.unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event|matches!(event,Event::RetainedRecord(record) if record.kind=="ocr_settings_changed" && record.data==json!({"revision":7}))));
    db.close().unwrap();
}

#[test]
fn rolls_back_invalid_import_without_writes() {
    let fixture = Fixture::new();
    let db = Connection::open(&fixture.path).unwrap();
    db.execute(
        "UPDATE knowledge_models SET body=?1",
        [b"{broken".as_slice()],
    )
    .unwrap();
    db.close().unwrap();
    let before = std::fs::read(&fixture.path).unwrap();
    assert!(Database::open(&fixture.path, fixture.node, None).is_err());
    assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
    assert!(!fixture.directory.path().join("backups").exists());
    let db = Connection::open_with_flags(&fixture.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    assert!(db.prepare("SELECT body FROM local_model_settings").is_err());
}
