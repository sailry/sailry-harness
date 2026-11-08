use super::*;
use crate::agent::{
    canonical::{self, Block, BlockKind, Compaction, Kind, Message, Operation, Record, Role},
    engine::conversation,
};
use crate::store::{
    agent::SessionQuery,
    database::counters::{self, Namespace},
};
use sailry_protocol::{TurnId, conversation::Part};
use serde_json::{Value, json};
use turso::params;

fn message(turn: TurnId, id: &str, role: Role, content: Vec<BlockKind>) -> Record {
    let mut record = Record::new(
        turn,
        Kind::Message(Message {
            role,
            part_count: content.len(),
            content: content
                .into_iter()
                .enumerate()
                .map(|(index, kind)| Block { index, kind })
                .collect(),
            ..Message::default()
        }),
    );
    record.id = id.into();
    record.provenance.author = match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
    .into();
    record
}

fn call(id: &str, metadata: Value) -> BlockKind {
    BlockKind::ToolRequest(canonical::ToolRequest {
        id: id.into(),
        name: "read_file".into(),
        arguments: json!({"path":"saved.txt","goose.executable":"literal argument"}),
        provider: Some(
            json!({"thoughtSignature":"retained-signature","goose.executable":false})
                .as_object()
                .unwrap()
                .clone(),
        ),
        tool_meta: Some(metadata),
        error: None,
    })
}

fn records(turn: TurnId) -> Vec<Record> {
    let old = message(
        turn,
        "before-summary",
        Role::User,
        vec![BlockKind::Text {
            text: "Earlier task".into(),
        }],
    );
    let mut summary = Record::new(
        turn,
        Kind::Compaction(Compaction {
            boundary: old.id.clone(),
            summary: Message {
                role: Role::User,
                part_count: 2,
                content: vec![
                    Block {
                        index: 0,
                        kind: BlockKind::Text {
                            text: "Saved summary".into(),
                        },
                    },
                    Block {
                        index: 1,
                        kind: call(
                            "summary-call",
                            json!({"goose.external_dispatch":true,"title":"Summary tool"}),
                        ),
                    },
                ],
                ..Message::default()
            },
            start: old.created,
            end: old.created,
            files: Vec::new(),
        }),
    );
    summary.id = "saved-summary".into();
    summary.provenance.author = "system".into();
    let input = message(
        turn,
        "current-input",
        Role::User,
        vec![BlockKind::Text {
            text: "Keep the literal goose.executable text".into(),
        }],
    );
    let requests = message(
        turn,
        "saved-calls",
        Role::Assistant,
        vec![
            call(
                "direct",
                json!({
                    "goose.executable":true,
                    "sailry.executable":true,
                    "title":"Saved request",
                    "foreign":{"goose.executable":false}
                }),
            ),
            call("patched", json!({"goose.executable":false})),
            call(
                "external",
                json!({"goose.external_dispatch":true,"goose.executable":false}),
            ),
        ],
    );
    let mut patch = Record::new(
        turn,
        Kind::Operation(Operation::ToolMeta {
            call: "patched".into(),
            patch: json!({"goose.executable":true,"extension":{"retained":[1,true]}}),
        }),
    );
    patch.id = "saved-approval".into();
    patch.provenance.author = "system".into();
    vec![old, summary, input, requests, patch]
}

async fn insert(db: &Connection, fixture: &Fixture, records: &[Record]) {
    let transaction = db.unchecked_transaction().await.unwrap();
    for record in records {
        let sequence = counters::next(&transaction, Namespace::AgentEvents)
            .await
            .unwrap();
        transaction
            .execute(
                "INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(?1,?2,?3,?4,?5)",
                params![
                    sequence as i64,
                    fixture.session.id.to_string(),
                    record.id.clone(),
                    record.turn.to_string(),
                    serde_json::to_vec(record).unwrap()
                ],
            )
            .await
            .unwrap();
        transaction
            .execute(
                "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)",
                params![fixture.session.id.to_string(), sequence as i64],
            )
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn stored(db: &Connection) -> Vec<(i64, Value)> {
    let mut rows = db
        .query(
            "SELECT sequence,body FROM agent_events ORDER BY sequence",
            (),
        )
        .await
        .unwrap();
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        records.push((
            row.get(0).unwrap(),
            serde_json::from_slice(&row.get::<Vec<u8>>(1).unwrap()).unwrap(),
        ));
    }
    records
}

fn expected(source: &[(i64, Value)]) -> Vec<(i64, Value)> {
    let mut expected = source.to_vec();
    assert_eq!(expected.len(), 5);
    expected[1].1["summary"]["content"][1]["tool_meta"] =
        json!({"sailry.external_dispatch":true,"title":"Summary tool"});
    expected[3].1["content"][0]["tool_meta"] = json!({
        "sailry.executable":true,
        "title":"Saved request",
        "foreign":{"goose.executable":false}
    });
    expected[3].1["content"][1]["tool_meta"] = json!({"sailry.executable":false});
    expected[3].1["content"][2]["tool_meta"] =
        json!({"sailry.external_dispatch":true,"sailry.executable":false});
    expected[4].1["patch"] = json!({"sailry.executable":true,"extension":{"retained":[1,true]}});
    expected
}

async fn verify(fixture: &Fixture, node: &Node, client: &Client) {
    fixture.verify(client, None).await;
    let Output::Conversation(history) = client
        .execute(client.prepare(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 20,
        }))
        .await
        .unwrap()
    else {
        panic!("conversation expected")
    };
    assert!(history.page.entries.iter().any(|entry| {
        entry.id == "current-input"
            && entry
                .parts
                .contains(&Part::Text("Keep the literal goose.executable text".into()))
    }));
    let requests = history
        .page
        .entries
        .iter()
        .find(|entry| entry.id == "saved-calls")
        .unwrap();
    assert_eq!(
        requests
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::ToolCall { id, .. } => id.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["direct", "patched", "external"]
    );
    let session = node
        .ingress
        .sessions(fixture.turn.id)
        .get(SessionQuery {
            session: fixture.session.id,
            recent: None,
            after: None,
        })
        .await
        .unwrap();
    let pending = conversation::pending(&session.conversation).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        ["direct", "patched"]
    );
    for call in pending {
        assert_eq!(conversation::request_executable(call), Some(true));
    }
}

#[tokio::test]
async fn preserves_history_and_execution_state() {
    let fixture = Fixture::new().await;
    let (handle, db) = open(&fixture.path()).await;
    insert(&db, &fixture, &records(fixture.turn.id)).await;
    db.execute("ALTER TABLE node DROP COLUMN tool_metadata_names", ())
        .await
        .unwrap();
    let original = stored(&db).await;
    let captures = super::captures(&db).await;
    let expected = expected(&original);
    close(handle, db).await;
    let (node, client) = fixture.reopen().await;
    verify(&fixture, &node, &client).await;
    drop(client);
    node.shutdown().await.unwrap();
    let backups = fixture.backups();
    assert_eq!(backups.len(), 1);
    let backup_bytes = std::fs::read(&backups[0]).unwrap();
    let (handle, db) = read(&backups[0]).await;
    assert_eq!(stored(&db).await, original);
    assert_eq!(super::captures(&db).await, captures);
    drop(db);
    drop(handle);
    let restored = schema::backup::preview(&backups[0]).unwrap();
    assert!(
        crate::store::database::upgrade_backup(restored.path(), fixture.node)
            .await
            .unwrap()
            .is_some()
    );
    let (handle, db) = read(restored.path()).await;
    assert_eq!(stored(&db).await, expected);
    assert_eq!(super::captures(&db).await, captures);
    crate::store::database::validate_backup(&db, fixture.node)
        .await
        .unwrap();
    drop(db);
    drop(handle);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), backup_bytes);
    let (handle, db) = open(&fixture.path()).await;
    assert_eq!(stored(&db).await, expected);
    assert_eq!(super::captures(&db).await, captures);
    crate::store::database::validate_backup(&db, fixture.node)
        .await
        .unwrap();
    close(handle, db).await;
    let (node, client) = fixture.reopen().await;
    verify(&fixture, &node, &client).await;
    drop(client);
    node.shutdown().await.unwrap();
    assert_eq!(fixture.backups(), backups);
    assert_eq!(std::fs::read(&backups[0]).unwrap(), backup_bytes);
    let (handle, db) = open(&fixture.path()).await;
    assert_eq!(stored(&db).await, expected);
    close(handle, db).await;
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn rejects_conflicting_states_without_changes() {
    let fixture = Fixture::new().await;
    let (handle, db) = open(&fixture.path()).await;
    let mut records = records(fixture.turn.id);
    let Kind::Operation(Operation::ToolMeta { patch, .. }) = &mut records[4].kind else {
        panic!("approval operation expected")
    };
    patch["sailry.executable"] = json!(false);
    insert(&db, &fixture, &records).await;
    db.execute("UPDATE node SET tool_metadata_names=0", ())
        .await
        .unwrap();
    let original = stored(&db).await;
    let captures = super::captures(&db).await;
    close(handle, db).await;
    let bytes = std::fs::read(fixture.path()).unwrap();
    let error = Node::start_empty(&fixture.profile)
        .await
        .err()
        .expect("conflicting metadata must reject the update");
    assert!(error.to_string().contains("tool"), "{error}");
    assert!(!error.to_string().contains("retained-signature"));
    assert_eq!(std::fs::read(fixture.path()).unwrap(), bytes);
    assert!(fixture.backups().is_empty());
    let (handle, db) = open(&fixture.path()).await;
    assert_eq!(stored(&db).await, original);
    assert_eq!(super::captures(&db).await, captures);
    close(handle, db).await;
    fixture.controller.close().await.unwrap();
}
