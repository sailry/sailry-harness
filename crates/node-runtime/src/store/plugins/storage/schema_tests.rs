use super::*;
use sailry_protocol::{NodeId, SessionId, plugin};
use serde_json::json;

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../schema.sql")).unwrap();
    db
}

fn package(db: &Connection, digest: &str, declaration: Declaration) -> plugin::Reference {
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
    .unwrap();
    db.execute("INSERT INTO plugins(name,revision,installed,body) VALUES('fixture',1,1,?1) ON CONFLICT(name) DO UPDATE SET body=excluded.body",
        [serde_json::to_vec(&info).unwrap()]).unwrap();
    reference
}

fn declared(scope: Scope, default: bool) -> Declaration {
    Declaration {
        collections: vec![storage::Collection {
            scope,
            key: Some("entry".into()),
            prefix: None,
            schema: json!({"type":"object","properties":{"title":{"type":"string"},"ready":{"type":"boolean","default":default}}}),
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

fn changes(db: &Connection) -> i64 {
    db.query_row("SELECT total_changes()", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn reads_captured_defaults_without_mutating_storage() {
    let db = database();
    let captured = package(&db, "first", declared(Scope::Node, false));
    package(&db, "second", declared(Scope::Node, true));
    let raw = json!({"title":"Old","legacy":7});
    let bytes = serde_json::to_vec(&raw).unwrap();
    write(&db, "fixture", "entry", &raw, &bytes, 0, None).unwrap();
    let before = changes(&db);
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
    .unwrap() else {
        panic!("value expected")
    };
    assert_eq!(entry.revision, 1);
    assert_eq!(entry.value, json!({"title":"Old","legacy":7,"ready":false}));
    assert_eq!(changes(&db), before);
    assert_eq!(read(&db, "fixture", "entry").unwrap().value, raw);
    let body: Vec<u8> = db
        .query_row(
            "SELECT value FROM plugin_values WHERE name='fixture' AND key='entry'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(body, bytes);
}

#[test]
fn revisioned_defaults_preserve_unknown_fields() {
    let db = database();
    let captured = package(&db, "first", declared(Scope::Node, false));
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
    let Output::PluginValue(entry) = execute(&db, &change).unwrap() else {
        panic!("value expected")
    };
    assert_eq!(entry.value, json!({"title":"New","ready":false,"legacy":7}));
    assert_eq!(entry.revision, 2);
    assert_eq!(
        execute(&db, &change).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    change.command = Command::WritePluginValue {
        key: "entry".into(),
        value: json!({"title":false}),
        expected_revision: 2,
    };
    let before = changes(&db);
    assert_eq!(
        execute(&db, &change).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(changes(&db), before);
    assert_eq!(read(&db, "fixture", "entry").unwrap(), entry);
    change.command = Command::WritePluginValue {
        key: "opaque".into(),
        value: json!([null, false, 1]),
        expected_revision: 0,
    };
    let Output::PluginValue(opaque) = execute(&db, &change).unwrap() else {
        panic!("value expected")
    };
    assert_eq!(opaque.value, json!([null, false, 1]));
}

#[test]
fn missing_and_deleted_values_remain_absent() {
    let db = database();
    let captured = package(&db, "first", declared(Scope::Node, false));
    let read_request = request(
        &captured,
        None,
        Command::ReadPluginValue {
            key: "entry".into(),
        },
    );
    let before = changes(&db);
    let Output::PluginValue(entry) = execute(&db, &read_request).unwrap() else {
        panic!("value expected")
    };
    assert!(!entry.present);
    assert_eq!(entry.revision, 0);
    assert_eq!(entry.value, Value::Null);
    assert_eq!(changes(&db), before);
    write(&db, "fixture", "entry", &json!({}), b"{}", 0, None).unwrap();
    remove(&db, "fixture", "entry", 1).unwrap();
    let before = changes(&db);
    let Output::PluginValue(entry) = execute(&db, &read_request).unwrap() else {
        panic!("value expected")
    };
    assert!(!entry.present);
    assert_eq!(entry.revision, 2);
    assert_eq!(entry.value, Value::Null);
    assert_eq!(changes(&db), before);
}

#[test]
fn search_projects_defaults_without_changing_indexes() {
    let db = database();
    let captured = package(&db, "first", declared(Scope::Node, false));
    let value = json!({"title":"Find"});
    let index = storage::Index {
        fields: ["Find".into(), String::new()],
        tags: vec![],
        order: 0,
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
    .unwrap();
    let before = changes(&db);
    let Output::PluginSearch(page) = execute(
        &db,
        &request(
            &captured,
            None,
            Command::SearchPluginValues(storage::Search {
                terms: vec!["Find".into()],
                weights: [1, 1],
                all: vec![],
                any: vec![],
                offset: 0,
                limit: 10,
            }),
        ),
    )
    .unwrap() else {
        panic!("search expected")
    };
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].value, json!({"title":"Find","ready":false}));
    assert_eq!(changes(&db), before);
    search::check(&db, "fixture").unwrap();
    assert_eq!(read(&db, "fixture", "entry").unwrap().value, value);
}

#[test]
fn conversation_reads_leave_immutable_records_unchanged() {
    let db = database();
    let captured = package(&db, "first", declared(Scope::Conversation, false));
    let session = SessionId::new();
    db.execute(
        "INSERT INTO sessions(id,revision) VALUES(?1,1)",
        [session.to_string()],
    )
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
    conversation::execute(&db, &change, None).unwrap();
    let rows = |db: &Connection| -> Vec<(i64, Vec<u8>)> {
        db.prepare("SELECT sequence,value FROM plugin_conversation_records ORDER BY sequence")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let original_rows = rows(&db);
    let before = changes(&db);
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
    .unwrap() else {
        panic!("conversation value expected")
    };
    assert_eq!(entry.value, json!({"title":"Old","legacy":7,"ready":false}));
    assert_eq!(entry.revision, 1);
    assert!(!entry.restored);
    assert_eq!(changes(&db), before);
    assert_eq!(rows(&db), original_rows);
    let Output::PluginConversationValue(saved) = execute(
        &db,
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
    .unwrap() else {
        panic!("conversation value expected")
    };
    assert_eq!(saved.value, json!({"title":"New","legacy":7,"ready":false}));
    assert_eq!(saved.revision, 2);
    assert_eq!(rows(&db)[0], original_rows[0]);
    assert_eq!(rows(&db).len(), 2);
}
