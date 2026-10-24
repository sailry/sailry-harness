use super::*;
use sailry_protocol::{NodeId, SessionId, plugin};
use serde_json::json;

async fn database() -> Connection {
    let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
        .await
        .unwrap();
    let db = crate::store::database::connect(&engine).await.unwrap();
    db.execute_batch(include_str!("../../schema.sql"))
        .await
        .unwrap();
    db
}

async fn package(db: &Connection, digest: &str, declaration: Declaration) -> plugin::Reference {
    let summary = plugin::Summary {
        name: "fixture".into(),
        revision: 1,
        digest: digest.into(),
        settings_revision: 0,
        enabled: true,
        version: None,
        description: None,
    };
    let reference = summary.reference();
    let extension = serde_json::from_value(json!({"api_version":"v1","actions":["storage.read","storage.write"],"storage":declaration})).unwrap();
    let info = plugin::Info {
        release_notes: None,
        summary,
        origin: None,
        icon: None,
        skill: None,
        mcp_source: None,
        extension: Some(extension),
        skills: vec![],
        mcp: vec![],
        settings: None,
        issues: vec![],
    };
    db.execute(
        "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
        params![digest, "fixture", serde_json::to_vec(&info).unwrap()],
    )
    .await
    .unwrap();
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES('fixture',1,1,?1) ON CONFLICT(name) DO UPDATE SET body=excluded.body",
        [serde_json::to_vec(&info).unwrap()]).await.unwrap();
    reference
}

fn declared(scope: Scope, default: bool) -> Declaration {
    Declaration {
        collections: vec![storage::Collection {
            scope,
            key: Some("entry".into()),
            prefix: None,
            schema: json!({"type":"object","properties":{"title":{"type":"string"},"ready":{"type":"boolean","default":default}}}),
            index: None,
        }],
    }
}

fn request(package: &plugin::Reference, session: Option<SessionId>, command: Command) -> Request {
    Request::new(NodeId([1; 32]), command).with_plugin(plugin::Context {
        package: package.clone(),
        session,
        worktree: None,
        turn: None,
        invocation: None,
        surface: Default::default(),
    })
}

async fn changes(db: &Connection) -> i64 {
    (async {
        let mut statement = db.prepare("SELECT total_changes()").await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn reads_captured_defaults_without_mutating_storage() {
    let db = database().await;
    let captured = package(&db, "first", declared(Scope::Node, false)).await;
    package(&db, "second", declared(Scope::Node, true)).await;
    let raw = json!({"title":"Old","legacy":7});
    let bytes = serde_json::to_vec(&raw).unwrap();
    write(&db, "fixture", "entry", &raw, &bytes, 0, None)
        .await
        .unwrap();
    let before = changes(&db).await;
    let Output::PluginValue(entry) = execute(
        &db,
        &request(
            &captured,
            None,
            Command::ReadPluginValue {
                key: "entry".into(),
            },
        ),
    )
    .await
    .unwrap() else {
        panic!("value expected")
    };
    assert_eq!(entry.revision, 1);
    assert_eq!(entry.value, json!({"title":"Old","legacy":7,"ready":false}));
    assert_eq!(changes(&db).await, before);
    assert_eq!(read(&db, "fixture", "entry").await.unwrap().value, raw);
    let body: Vec<u8> = (async {
        let mut statement = db
            .prepare("SELECT value FROM plugin_values WHERE name='fixture' AND key='entry'")
            .await?;
        let row = statement.query_row(()).await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped
    })
    .await
    .unwrap();
    assert_eq!(body, bytes);
}

#[tokio::test]
async fn revisioned_defaults_preserve_unknown_fields() {
    let db = database().await;
    let captured = package(&db, "first", declared(Scope::Node, false)).await;
    let old = json!({"title":"Old","legacy":7});
    write(
        &db,
        "fixture",
        "entry",
        &old,
        &serde_json::to_vec(&old).unwrap(),
        0,
        None,
    )
    .await
    .unwrap();
    let mut change = request(
        &captured,
        None,
        Command::WritePluginValue {
            key: "entry".into(),
            value: json!({"title":"New"}),
            expected_revision: 1,
        },
    );
    let Output::PluginValue(entry) = execute(&db, &change).await.unwrap() else {
        panic!("value expected")
    };
    assert_eq!(entry.value, json!({"title":"New","ready":false,"legacy":7}));
    assert_eq!(entry.revision, 2);
    assert_eq!(
        execute(&db, &change).await.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    change.command = Command::WritePluginValue {
        key: "entry".into(),
        value: json!({"title":false}),
        expected_revision: 2,
    };
    let before = changes(&db).await;
    assert_eq!(
        execute(&db, &change).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(changes(&db).await, before);
    assert_eq!(read(&db, "fixture", "entry").await.unwrap(), entry);
    change.command = Command::WritePluginValue {
        key: "opaque".into(),
        value: json!([null, false, 1]),
        expected_revision: 0,
    };
    let Output::PluginValue(opaque) = execute(&db, &change).await.unwrap() else {
        panic!("value expected")
    };
    assert_eq!(opaque.value, json!([null, false, 1]));
}

#[tokio::test]
async fn missing_and_deleted_values_remain_absent() {
    let db = database().await;
    let captured = package(&db, "first", declared(Scope::Node, false)).await;
    let read_request = request(
        &captured,
        None,
        Command::ReadPluginValue {
            key: "entry".into(),
        },
    );
    let before = changes(&db).await;
    let Output::PluginValue(entry) = execute(&db, &read_request).await.unwrap() else {
        panic!("value expected")
    };
    assert!(!entry.present);
    assert_eq!(entry.revision, 0);
    assert_eq!(entry.value, Value::Null);
    assert_eq!(changes(&db).await, before);
    write(&db, "fixture", "entry", &json!({}), b"{}", 0, None)
        .await
        .unwrap();
    remove(&db, "fixture", "entry", 1).await.unwrap();
    let before = changes(&db).await;
    let Output::PluginValue(entry) = execute(&db, &read_request).await.unwrap() else {
        panic!("value expected")
    };
    assert!(!entry.present);
    assert_eq!(entry.revision, 2);
    assert_eq!(entry.value, Value::Null);
    assert_eq!(changes(&db).await, before);
}

#[tokio::test]
async fn search_projects_defaults_without_changing_indexes() {
    let db = database().await;
    let captured = package(&db, "first", declared(Scope::Node, false)).await;
    let value = json!({"title":"Find"});
    let index = storage::Index {
        fields: ["Find".into(), String::new()],
        tags: vec![],
        order: 0,
        source: None,
    };
    write(
        &db,
        "fixture",
        "entry",
        &value,
        &serde_json::to_vec(&value).unwrap(),
        0,
        Some(&index),
    )
    .await
    .unwrap();
    let before = changes(&db).await;
    let Output::PluginSearch(page) = execute(
        &db,
        &request(
            &captured,
            None,
            Command::SearchPluginValues(storage::Search {
                terms: vec!["Find".into()],
                query: None,
                mode: storage::Mode::Keyword,
                weights: [1, 1],
                all: vec![],
                any: vec![],
                offset: 0,
                limit: 10,
            }),
        ),
    )
    .await
    .unwrap() else {
        panic!("search expected")
    };
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].value, json!({"title":"Find","ready":false}));
    assert_eq!(changes(&db).await, before);
    search::check(&db, "fixture").await.unwrap();
    assert_eq!(read(&db, "fixture", "entry").await.unwrap().value, value);
}

#[tokio::test]
async fn conversation_reads_leave_immutable_records_unchanged() {
    let mut db = database().await;
    let captured = package(&db, "first", declared(Scope::Conversation, false)).await;
    let session = SessionId::new();
    let transaction = db.transaction().await.unwrap();
    let row = crate::store::database::counters::next(
        &transaction,
        crate::store::database::counters::Namespace::Sessions,
    )
    .await
    .unwrap();
    transaction
        .execute(
            "INSERT INTO sessions(rowid,id,revision) VALUES(?1,?2,1)",
            params![row as i64, session.to_string()],
        )
        .await
        .unwrap();
    let original = json!({"title":"Old","legacy":7});
    let change = request(
        &captured,
        Some(session),
        Command::WritePluginConversationValue {
            key: "entry".into(),
            value: original.clone(),
            expected_revision: 0,
        },
    );
    // Seed an opaque checkpoint as an older definition would have written it.
    conversation::execute(&transaction, &change, None)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let rows = async |db: &Connection| -> Vec<(i64, Vec<u8>)> {
        (async {
            let mut rows = db
                .prepare("SELECT sequence,value FROM plugin_conversation_records ORDER BY sequence")
                .await
                .unwrap()
                .query(())
                .await?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await? {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                values.push(mapped);
            }
            Ok::<_, turso::Error>(values.into_iter())
        })
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect()
    };
    let original_rows = rows(&db).await;
    let before = changes(&db).await;
    let Output::PluginConversationValue(entry) = execute(
        &db,
        &request(
            &captured,
            Some(session),
            Command::ReadPluginConversationValue {
                key: "entry".into(),
            },
        ),
    )
    .await
    .unwrap() else {
        panic!("conversation value expected")
    };
    assert_eq!(entry.value, json!({"title":"Old","legacy":7,"ready":false}));
    assert_eq!(entry.revision, 1);
    assert!(!entry.restored);
    assert_eq!(changes(&db).await, before);
    assert_eq!(rows(&db).await, original_rows);
    let transaction = db.transaction().await.unwrap();
    let Output::PluginConversationValue(saved) = execute(
        &transaction,
        &request(
            &captured,
            Some(session),
            Command::WritePluginConversationValue {
                key: "entry".into(),
                value: json!({"title":"New"}),
                expected_revision: 1,
            },
        ),
    )
    .await
    .unwrap() else {
        panic!("conversation value expected")
    };
    transaction.commit().await.unwrap();
    assert_eq!(saved.value, json!({"title":"New","legacy":7,"ready":false}));
    assert_eq!(saved.revision, 2);
    assert_eq!(rows(&db).await[0], original_rows[0]);
    assert_eq!(rows(&db).await.len(), 2);
}
