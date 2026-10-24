use super::*;
use crate::store::knowledge::Capture;
use sailry_protocol::{KnowledgeId, SessionId, WorktreeId, knowledge, plugin};
use serde_json::json;

async fn fixture(scope: Scope, prefix: &str) -> (Connection, Stored, CancellationToken) {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    let transaction = db.unchecked_transaction().await.unwrap();
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
        transaction
            .execute(
                "INSERT INTO worktrees(id,path,main) VALUES(?1,'fixture/worktree',1)",
                [stored.capture.worktree.unwrap().to_string()],
            )
            .await
            .unwrap();
        let rowid = crate::store::database::counters::next(
            &transaction,
            crate::store::database::counters::Namespace::Sessions,
        )
        .await
        .unwrap();
        transaction
            .execute(
                "INSERT INTO sessions(rowid,id,worktree,revision) VALUES(?1,?2,?3,1)",
                params![
                    rowid as i64,
                    stored.capture.session.unwrap().to_string(),
                    stored.capture.worktree.unwrap().to_string(),
                ],
            )
            .await
            .unwrap();
    }
    collections::persist(&transaction, &stored).await.unwrap();
    transaction.commit().await.unwrap();
    (db, stored, CancellationToken::new())
}

async fn value(
    db: &Connection,
    name: &str,
    key: &str,
    revision: u64,
    body: Value,
    index: Option<storage::Index>,
) {
    db.execute("INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data",
        params![name, key, revision as i64, serde_json::to_vec(&body).unwrap(), index.map(|index| serde_json::to_vec(&index).unwrap())]).await.unwrap();
}

async fn conversation(db: &Connection, session: SessionId, name: &str, key: &str, body: Value) {
    use crate::store::database::counters::{self, Namespace};
    let transaction = db.unchecked_transaction().await.unwrap();
    let present = transaction
        .prepare("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)")
        .await
        .unwrap()
        .query_row([session.to_string()])
        .await
        .unwrap()
        .get::<bool>(0)
        .unwrap();
    if !present {
        let rowid = counters::next(&transaction, Namespace::Sessions)
            .await
            .unwrap();
        transaction
            .execute(
                "INSERT INTO sessions(rowid,id,revision) VALUES(?1,?2,1)",
                params![rowid as i64, session.to_string()],
            )
            .await
            .unwrap();
    }
    let sequence = counters::next(&transaction, Namespace::PluginRecords)
        .await
        .unwrap();
    transaction
        .execute(
            "INSERT INTO plugin_conversation_records(sequence,name,key,value) VALUES(?1,?2,?3,?4)",
            params![
                sequence as i64,
                name,
                key,
                serde_json::to_vec(&body).unwrap()
            ],
        )
        .await
        .unwrap();
    transaction.execute("INSERT INTO plugin_conversation_values(name,session,key,revision,record,restored) VALUES(?1,?2,?3,1,?4,0)", params![name,session.to_string(),key,sequence as i64]).await.unwrap();
    transaction.commit().await.unwrap();
}

#[tokio::test]
async fn pages_raw_values_in_captured_namespace() {
    let (db, stored, stop) = fixture(Scope::Node, "docs/%").await;
    for (key, body) in [
        ("docs/%a", json!("我们使用向量数据库")),
        (
            "docs/%b",
            json!({"title":"Original title","body":"完整文本"}),
        ),
        ("docs/other", json!("Excluded")),
    ] {
        value(&db, "corpus", key, 1, body, None).await;
    }
    value(&db, "other", "docs/%a", 1, json!("Private"), None).await;
    let first = page(&db, &stored, None, 1, &stop).await.unwrap();
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].key, "docs/%a");
    assert_eq!(first.records[0].body, "我们使用向量数据库");
    assert_eq!(first.records[0].source.pointer, "");
    assert_eq!(first.after.as_deref(), Some("docs/%a"));
    let next = page(&db, &stored, first.after.as_deref(), 16, &stop)
        .await
        .unwrap();
    assert!(next.after.is_none());
    assert_eq!(next.records.len(), 1);
    assert_eq!(next.records[0].title, "Original title");
    assert_eq!(
        serde_json::from_str::<Value>(&next.records[0].body).unwrap(),
        json!({"title":"Original title","body":"完整文本"})
    );
    assert!(
        unchanged(&db, &stored, &next.records[0], &stop)
            .await
            .unwrap()
    );
    assert_eq!(
        record(&db, &stored, "docs/%b", &stop)
            .await
            .unwrap()
            .unwrap()
            .body,
        next.records[0].body
    );
    assert_eq!(
        record(&db, &stored, "docs/other", &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[tokio::test]
async fn follows_original_pointer_and_rechecks_both_records() {
    let (db, stored, stop) = fixture(Scope::Node, "entry/").await;
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
    )
    .await;
    value(&db, "corpus", "body/raw", 3, original.clone(), None).await;
    let record = page(&db, &stored, None, 16, &stop)
        .await
        .unwrap()
        .records
        .remove(0);
    assert_eq!(record.body, "Raw original 向量 text");
    assert_eq!(record.title, "Actual metadata");
    assert_eq!(record.tags, ["active"]);
    assert_eq!(record.source.key, "body/raw");
    assert_eq!(record.source_revision, 3);
    assert!(unchanged(&db, &stored, &record, &stop).await.unwrap());
    for (revision, body, expected) in [
        (3, json!({"text":"Changed without revision"}), false),
        (4, original.clone(), false),
        (3, original, true),
    ] {
        value(&db, "corpus", "body/raw", revision, body, None).await;
        assert_eq!(
            unchanged(&db, &stored, &record, &stop).await.unwrap(),
            expected
        );
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
    )
    .await;
    assert!(!unchanged(&db, &stored, &record, &stop).await.unwrap());
    value(&db, "corpus", "entry/one", 3, metadata, Some(index)).await;
    assert!(!unchanged(&db, &stored, &record, &stop).await.unwrap());
}

#[tokio::test]
async fn confines_conversation_sources_to_captured_session() {
    let (db, stored, stop) = fixture(Scope::Conversation, "notes/").await;
    conversation(
        &db,
        stored.capture.session.unwrap(),
        "corpus",
        "notes/raw",
        json!("Captured conversation"),
    )
    .await;
    conversation(
        &db,
        SessionId::new(),
        "corpus",
        "notes/raw",
        json!("Other conversation"),
    )
    .await;
    conversation(
        &db,
        stored.capture.session.unwrap(),
        "other",
        "notes/raw",
        json!("Other package"),
    )
    .await;
    value(&db, "corpus", "notes/raw", 1, json!("Node data"), None).await;
    let records = page(&db, &stored, None, 16, &stop).await.unwrap().records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].body, "Captured conversation");
    assert!(unchanged(&db, &stored, &records[0], &stop).await.unwrap());
}

#[tokio::test]
async fn rejects_stale_definitions_and_cancels_reads() {
    let (db, stored, stop) = fixture(Scope::Node, "").await;
    value(&db, "corpus", "raw", 1, json!("Original"), None).await;
    let record = page(&db, &stored, None, 16, &stop)
        .await
        .unwrap()
        .records
        .remove(0);
    let mut progress = stored.clone();
    progress.collection.progress.status = knowledge::Status::Ready;
    progress.collection.progress.sources = 1;
    progress.collection.progress.completed = 1;
    collections::persist(&db, &progress).await.unwrap();
    assert!(unchanged(&db, &stored, &record, &stop).await.unwrap());
    collections::advance(&mut progress).unwrap();
    collections::persist(&db, &progress).await.unwrap();
    assert!(!unchanged(&db, &stored, &record, &stop).await.unwrap());
    assert_eq!(
        page(&db, &stored, None, 16, &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
    for limit in [0, 17] {
        assert_eq!(
            page(&db, &progress, None, limit, &stop)
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    stop.cancel();
    assert_eq!(
        page(&db, &progress, None, 16, &stop)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        unchanged(&db, &progress, &record, &stop)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[tokio::test]
async fn skips_missing_sources_without_stalling_cursor() {
    let (db, stored, stop) = fixture(Scope::Node, "entry/").await;
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
    )
    .await;
    value(&db, "corpus", "entry/b", 1, json!("Original"), None).await;
    let first = page(&db, &stored, None, 1, &stop).await.unwrap();
    assert!(first.records.is_empty());
    assert_eq!(first.after.as_deref(), Some("entry/a"));
    let next = page(&db, &stored, first.after.as_deref(), 1, &stop)
        .await
        .unwrap();
    assert_eq!(next.records[0].body, "Original");
    assert!(next.after.is_none());
}
