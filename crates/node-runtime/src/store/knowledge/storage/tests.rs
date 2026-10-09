use super::*;
use crate::store::knowledge::Capture;
use sailry_protocol::{KnowledgeId, SessionId, WorktreeId, knowledge, plugin};
use serde_json::json;

fn fixture(scope: Scope, prefix: &str) -> (Connection, Stored, CancellationToken) {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    let conversation = scope == Scope::Conversation;
    let stored = Stored {
        collection: knowledge::Collection {
            id: KnowledgeId::new(),
            revision: 1,
            rebuild: 0,
            paused: false,
            owner: knowledge::Owner::Plugin {
                name: "corpus".into(),
            },
            definition: knowledge::Definition {
                name: "Raw corpus".into(),
                source: Source::Storage {
                    scope,
                    prefix: prefix.into(),
                },
                model: None,
                globs: vec![],
                tags: vec![],
                shared_with: vec![],
            },
            progress: knowledge::Progress {
                generation: 1,
                ..Default::default()
            },
        },
        capture: Capture {
            package: Some(plugin::Reference {
                name: "corpus".into(),
                digest: "a".repeat(64),
                settings_revision: 0,
            }),
            worktree: conversation.then(WorktreeId::new),
            session: conversation.then(SessionId::new),
        },
        automatic: false,
    };
    if conversation {
        db.execute(
            "INSERT INTO worktrees(id,path,main) VALUES(?1,'fixture/worktree',1)",
            [stored.capture.worktree.unwrap().to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sessions(id,worktree,revision) VALUES(?1,?2,1)",
            params![
                stored.capture.session.unwrap().to_string(),
                stored.capture.worktree.unwrap().to_string(),
            ],
        )
        .unwrap();
    }
    collections::persist(&db, &stored).unwrap();
    (db, stored, CancellationToken::new())
}

fn value(
    db: &Connection,
    name: &str,
    key: &str,
    revision: u64,
    body: Value,
    index: Option<storage::Index>,
) {
    db.execute("INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data",
        params![name, key, revision as i64, serde_json::to_vec(&body).unwrap(), index.map(|index| serde_json::to_vec(&index).unwrap())]).unwrap();
}

fn conversation(db: &Connection, session: SessionId, name: &str, key: &str, body: Value) {
    db.execute(
        "INSERT INTO sessions(id,revision) VALUES(?1,1) ON CONFLICT(id) DO NOTHING",
        [session.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO plugin_conversation_records(name,key,value) VALUES(?1,?2,?3)",
        params![name, key, serde_json::to_vec(&body).unwrap()],
    )
    .unwrap();
    let sequence = db.last_insert_rowid();
    db.execute("INSERT INTO plugin_conversation_values(name,session,key,revision,record,restored) VALUES(?1,?2,?3,1,?4,0)", params![name,session.to_string(),key,sequence]).unwrap();
}

#[test]
fn pages_raw_values_in_captured_namespace() {
    let (db, stored, stop) = fixture(Scope::Node, "docs/%");
    for (key, body) in [
        ("docs/%a", json!("我们使用向量数据库")),
        (
            "docs/%b",
            json!({"title":"Original title","body":"完整文本"}),
        ),
        ("docs/other", json!("Excluded")),
    ] {
        value(&db, "corpus", key, 1, body, None);
    }
    value(&db, "other", "docs/%a", 1, json!("Private"), None);
    let first = page(&db, &stored, None, 1, &stop).unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].key, "docs/%a");
    assert_eq!(first.records[0].body, "我们使用向量数据库");
    assert_eq!(first.records[0].source.pointer, "");
    assert_eq!(first.after.as_deref(), Some("docs/%a"));
    let next = page(&db, &stored, first.after.as_deref(), 16, &stop).unwrap();
    assert!(next.after.is_none());
    assert_eq!(next.records.len(), 1);
    assert_eq!(next.records[0].title, "Original title");
    assert_eq!(
        serde_json::from_str::<Value>(&next.records[0].body).unwrap(),
        json!({"title":"Original title","body":"完整文本"})
    );
    assert!(unchanged(&db, &stored, &next.records[0], &stop).unwrap());
    assert_eq!(
        record(&db, &stored, "docs/%b", &stop)
            .unwrap()
            .unwrap()
            .body,
        next.records[0].body
    );
    assert_eq!(
        record(&db, &stored, "docs/other", &stop).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn follows_original_pointer_and_rechecks_both_records() {
    let (db, stored, stop) = fixture(Scope::Node, "entry/");
    let index = storage::Index {
        fields: ["Precut title".into(), "Precut body".into()],
        tags: vec!["active".into()],
        order: 0,
        source: Some(storage::Source {
            key: "body/raw".into(),
            pointer: "/text".into(),
        }),
    };
    let metadata = json!({"title":"Actual metadata"});
    let original = json!({"text":"Raw original 向量 text"});
    value(
        &db,
        "corpus",
        "entry/one",
        2,
        metadata.clone(),
        Some(index.clone()),
    );
    value(&db, "corpus", "body/raw", 3, original.clone(), None);
    let record = page(&db, &stored, None, 16, &stop)
        .unwrap()
        .records
        .remove(0);
    assert_eq!(record.body, "Raw original 向量 text");
    assert_eq!(record.title, "Actual metadata");
    assert_eq!(record.tags, ["active"]);
    assert_eq!(record.source.key, "body/raw");
    assert_eq!(record.source_revision, 3);
    assert!(unchanged(&db, &stored, &record, &stop).unwrap());
    for (revision, body, expected) in [
        (3, json!({"text":"Changed without revision"}), false),
        (4, original.clone(), false),
        (3, original, true),
    ] {
        value(&db, "corpus", "body/raw", revision, body, None);
        assert_eq!(unchanged(&db, &stored, &record, &stop).unwrap(), expected);
    }
    value(
        &db,
        "corpus",
        "entry/one",
        2,
        metadata.clone(),
        Some(storage::Index {
            tags: vec!["changed".into()],
            ..index.clone()
        }),
    );
    assert!(!unchanged(&db, &stored, &record, &stop).unwrap());
    value(&db, "corpus", "entry/one", 3, metadata, Some(index));
    assert!(!unchanged(&db, &stored, &record, &stop).unwrap());
}

#[test]
fn confines_conversation_sources_to_captured_session() {
    let (db, stored, stop) = fixture(Scope::Conversation, "notes/");
    conversation(
        &db,
        stored.capture.session.unwrap(),
        "corpus",
        "notes/raw",
        json!("Captured conversation"),
    );
    conversation(
        &db,
        SessionId::new(),
        "corpus",
        "notes/raw",
        json!("Other conversation"),
    );
    conversation(
        &db,
        stored.capture.session.unwrap(),
        "other",
        "notes/raw",
        json!("Other package"),
    );
    value(&db, "corpus", "notes/raw", 1, json!("Node data"), None);
    let records = page(&db, &stored, None, 16, &stop).unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].body, "Captured conversation");
    assert!(unchanged(&db, &stored, &records[0], &stop).unwrap());
}

#[test]
fn rejects_stale_definitions_and_cancels_reads() {
    let (db, stored, stop) = fixture(Scope::Node, "");
    value(&db, "corpus", "raw", 1, json!("Original"), None);
    let record = page(&db, &stored, None, 16, &stop)
        .unwrap()
        .records
        .remove(0);
    let mut progress = stored.clone();
    progress.collection.progress.status = knowledge::Status::Ready;
    progress.collection.progress.sources = 1;
    progress.collection.progress.completed = 1;
    collections::persist(&db, &progress).unwrap();
    assert!(unchanged(&db, &stored, &record, &stop).unwrap());
    collections::advance(&mut progress).unwrap();
    collections::persist(&db, &progress).unwrap();
    assert!(!unchanged(&db, &stored, &record, &stop).unwrap());
    assert_eq!(
        page(&db, &stored, None, 16, &stop).err().unwrap().code,
        ErrorCode::RevisionConflict
    );
    for limit in [0, 17] {
        assert_eq!(
            page(&db, &progress, None, limit, &stop).err().unwrap().code,
            ErrorCode::InvalidRequest
        );
    }
    stop.cancel();
    assert_eq!(
        page(&db, &progress, None, 16, &stop).err().unwrap().code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        unchanged(&db, &progress, &record, &stop).unwrap_err().code,
        ErrorCode::Cancelled
    );
}

#[test]
fn skips_missing_sources_without_stalling_cursor() {
    let (db, stored, stop) = fixture(Scope::Node, "entry/");
    value(
        &db,
        "corpus",
        "entry/a",
        1,
        json!({"title":"Missing"}),
        Some(storage::Index {
            fields: ["".into(), "".into()],
            tags: vec![],
            order: 0,
            source: Some(storage::Source {
                key: "missing".into(),
                pointer: "/text".into(),
            }),
        }),
    );
    value(&db, "corpus", "entry/b", 1, json!("Original"), None);
    let first = page(&db, &stored, None, 1, &stop).unwrap();
    assert!(first.records.is_empty());
    assert_eq!(first.after.as_deref(), Some("entry/a"));
    let next = page(&db, &stored, first.after.as_deref(), 1, &stop).unwrap();
    assert_eq!(next.records[0].body, "Original");
    assert!(next.after.is_none());
}
