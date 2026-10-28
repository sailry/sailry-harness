use super::*;
use crate::agent::canonical::{self as records, BlockKind, Kind, Record, Role};
use fixture::{message, record, request, response};
use serde_json::json;

async fn captures(fixture: &fixture::Fixture) -> String {
    fixture
        .database
        .connection
        .prepare(
            "SELECT json_group_array(json_object('id',t.id,'plugins',hex(t.plugins),
         'roles',hex(t.roles),'instructions',hex(t.instructions),'revision',t.revision,
         'config',hex(r.config),'captured_roles',hex(r.roles)))
         FROM (SELECT * FROM turns ORDER BY id) t JOIN session_revisions r
         ON r.session=t.session AND r.revision=t.revision",
        )
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap()
}

#[tokio::test]
async fn keeps_configuration_out_of_messages() {
    let mut fixture = fixture::Fixture::new().await;
    let context = record(
        "system",
        Kind::Configuration(records::Configuration {
            context: records::Context {
                captured: true,
                personal: Some("Keep the captured policy".into()),
                ..Default::default()
            },
        }),
    );
    let ordinary = message(
        "system",
        Role::User,
        vec![BlockKind::Text {
            text: "Other system content".into(),
        }],
    );
    let turn = fixture.append([context.clone(), ordinary.clone()]).await;
    let restored = sessions::get(
        &fixture.database.connection,
        turn,
        sessions::Query {
            session: fixture.session,
            recent: None,
            after: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(restored.records.len(), 2);
    assert_eq!(restored.records[0].id, context.id);
    assert_eq!(
        restored.context.personal.as_deref(),
        Some("Keep the captured policy")
    );
    assert_eq!(restored.conversation.len(), 1);
    assert_eq!(
        restored.conversation.messages()[0].id.as_deref(),
        Some(ordinary.id.as_str())
    );
    assert_eq!(
        restored.conversation.messages()[0].as_concat_text(),
        "Other system content"
    );
    assert!(
        fixture
            .read(None, 100)
            .await
            .page
            .entries
            .iter()
            .find(|entry| entry.id == context.id)
            .unwrap()
            .parts
            .is_empty()
    );
}

async fn body(db: &turso::Connection, id: &str) -> Vec<u8> {
    db.prepare("SELECT body FROM agent_events WHERE id=?1")
        .await
        .unwrap()
        .query_row([id])
        .await
        .unwrap()
        .get(0)
        .unwrap()
}

async fn legacy(db: &turso::Connection, records: &[(TurnId, String, Vec<u8>)]) {
    for (turn, id, body) in records {
        db.execute(
            "UPDATE agent_events SET body=?1 WHERE turn=?2 AND id=?3",
            params![body.as_slice(), turn.to_string(), id.as_str()],
        )
        .await
        .unwrap();
    }
    db.execute_batch("DROP INDEX agent_usage; DROP INDEX agent_usage_time; DROP TABLE agent_blobs")
        .await
        .unwrap();
    db.execute_batch(include_str!("../../database/schema/upgrades/usage.sql"))
        .await
        .unwrap();
}

#[tokio::test]
async fn upgrades_history_with_private_backup() {
    let mut fixture = fixture::Fixture::new().await;
    fixture::files(&fixture.database, fixture._root.path()).await;
    let mut answer = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Text {
            text: "Recorded answer".into(),
        }],
    );
    answer.provenance.branch = "recorded-scope".into();
    answer.message_mut().unwrap().usage = Some(records::Usage {
        input: Some(11),
        output: Some(3),
        total: Some(14),
        ..Default::default()
    });
    let answer_body=serde_json::to_vec_pretty(&json!({"id":answer.id,"timestamp":answer.created,"author":"assistant","branch":"recorded-scope","turn_complete":false,"interrupted":false,"content":{"role":"model","parts":[{"text":"Recorded answer"}]},"usage_metadata":{"prompt_token_count":11,"candidates_token_count":3,"total_token_count":14},"actions":{"state_delta":{},"artifact_delta":{}}})).unwrap();
    let first = fixture.append([answer.clone()]).await;
    let mut call = message(
        "assistant",
        Role::Assistant,
        vec![
            BlockKind::Text {
                text: "Before the call".into(),
            },
            BlockKind::ToolRequest(request(
                Some("recorded-call"),
                "read_file",
                json!({"path":"recorded.txt"}),
            )),
        ],
    );
    call.provenance.branch = "recorded-scope".into();
    if let BlockKind::ToolRequest(request) = &mut call.message_mut().unwrap().content[1].kind {
        request.signature = Some("recorded-signature".into());
        request.provider =
            Some(serde_json::from_value(json!({"thoughtSignature":"recorded-signature"})).unwrap());
    }
    let call_body=serde_json::to_vec_pretty(&json!({"id":call.id,"timestamp":call.created,"author":"assistant","branch":"recorded-scope","turn_complete":false,"interrupted":false,"content":{"role":"model","parts":[{"text":"Before the call"},{"id":"recorded-call","name":"read_file","args":{"path":"recorded.txt"},"thought_signature":"recorded-signature"}]},"actions":{"state_delta":{},"artifact_delta":{}}})).unwrap();
    let original = json!({"content":"Recorded tool result","image":{"mimeType":"image/png","data":"AAEC/f7/"}});
    let bytes = vec![0, 1, 2, 253, 254, 255];
    let mut output = response(Some("recorded-call"), "read_file", original.clone());
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    output.result = super::super::blobs::capture(&transaction, original.clone())
        .await
        .unwrap();
    output.attachments.push(records::Attachment::Inline {
        asset: super::super::blobs::put(&transaction, "image/png", &bytes)
            .await
            .unwrap(),
    });
    transaction.commit().await.unwrap();
    let mut result = message(
        "tool",
        Role::User,
        vec![
            BlockKind::Text {
                text: "Before the result".into(),
            },
            BlockKind::ToolResponse(output),
        ],
    );
    result.provenance.branch = call.provenance.branch.clone();
    result.metadata.context = Some(records::Context {
        personal: Some("Keep the captured policy".into()),
        ..Default::default()
    });
    let result_body=serde_json::to_vec_pretty(&json!({"id":result.id,"timestamp":result.created,"author":"tool","branch":"recorded-scope","turn_complete":false,"interrupted":false,"content":{"role":"user","parts":[{"text":"Before the result"},{"id":"recorded-call","functionResponse":{"name":"read_file","response":original,"inline_data":[{"mime_type":"image/png","data":bytes}]}}]},"actions":{"state_delta":{"sailry.context.personal":"Keep the captured policy"},"artifact_delta":{}}})).unwrap();
    let exchange = fixture.append([call.clone(), result.clone()]).await;
    let summary = record(
        "system",
        Kind::Compaction(records::Compaction {
            boundary: result.id.clone(),
            summary: records::Message {
                role: Role::User,
                part_count: 1,
                content: vec![records::Block {
                    index: 0,
                    kind: BlockKind::Text {
                        text: "Recorded summary".into(),
                    },
                }],
                ..Default::default()
            },
            start: call.created,
            end: result.created,
            files: Vec::new(),
        }),
    );
    let compacted = fixture.append([summary.clone()]).await;
    let summary_body=serde_json::to_vec_pretty(&json!({"id":summary.id,"timestamp":summary.created,"author":"system","branch":"","turn_complete":false,"interrupted":false,"event_metadata":{"sailry.context.end_event":result.id},"actions":{"state_delta":{},"artifact_delta":{},"compaction":{"start_timestamp":call.created,"end_timestamp":result.created,"compacted_content":{"role":"model","parts":[{"text":"Recorded summary"}]}}}})).unwrap();
    let originals = vec![
        (first, answer.id.clone(), answer_body),
        (exchange, call.id.clone(), call_body),
        (exchange, result.id.clone(), result_body),
        (compacted, summary.id.clone(), summary_body),
    ];
    let queue = queue::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let session = fixture.session;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetQueuePaused {
            session,
            expected_revision: queue.revision,
            paused: true,
        },
    )
    .await;
    let captured = captures(&fixture).await;
    let projected = fixture.read(None, 100).await;
    let entry = projected
        .page
        .entries
        .iter()
        .find(|entry| entry.id == result.id)
        .unwrap();
    let Part::ToolResult {
        id,
        summary: detail,
        images,
        ..
    } = &entry.parts[1]
    else {
        panic!("recorded tool result expected")
    };
    assert_eq!(images[0].part, 1);
    let image = images[0].clone();
    let reference = sailry_protocol::tool::OutputReference {
        call: id.clone().unwrap(),
        revision: blake3::hash(original.to_string().as_bytes())
            .to_hex()
            .to_string(),
    };
    let detail_revision = detail.revision.clone();
    legacy(&fixture.database.connection, &originals).await;
    let path = fixture.database.path.clone();
    let fixture::Fixture {
        database,
        events,
        session,
        _root,
    } = fixture;
    database.close().await.unwrap();
    let database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
    let mut fixture = fixture::Fixture {
        database,
        events,
        session,
        _root,
    };
    assert_eq!(fixture.read(None, 100).await, projected);
    assert_eq!(captures(&fixture).await, captured);
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &image)
            .await
            .unwrap(),
        bytes
    );
    let record: Record =
        serde_json::from_slice(&body(&fixture.database.connection, &result.id).await).unwrap();
    let BlockKind::ToolResponse(response) = &record.message().unwrap().content[1].kind else {
        panic!("canonical result expected")
    };
    assert_eq!(record.provenance.author, result.provenance.author);
    assert_eq!(record.provenance.branch, result.provenance.branch);
    assert_eq!(response.revision, reference.revision);
    assert!(response.result.value["image"]["data"].is_null());
    assert_eq!(
        super::super::blobs::hydrate(&fixture.database.connection, &response.result)
            .await
            .unwrap(),
        original
    );
    let restored = sessions::get(
        &fixture.database.connection,
        compacted,
        sessions::Query {
            session: fixture.session,
            recent: None,
            after: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(restored.records.len(), 4);
    assert_eq!(restored.conversation.agent_visible_messages().len(), 1);
    let visible = restored.conversation.agent_visible_messages();
    assert_eq!(visible[0].id.as_deref(), Some(summary.id.as_str()));
    assert_eq!(visible[0].role, rmcp::model::Role::User);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await, projected);
    let current = fixture.read(None, 100).await;
    let entry = current
        .page
        .entries
        .iter()
        .find(|entry| entry.id == result.id)
        .unwrap();
    assert!(
        matches!(&entry.parts[1], Part::ToolResult { summary, .. } if summary.revision == detail_revision)
    );
    let backups: Vec<_> = std::fs::read_dir(fixture._root.path().join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("node.turso"))
        .collect();
    assert_eq!(backups.len(), 1);
    let backup = crate::store::database::engine(&backups[0]).await.unwrap();
    let connection = crate::store::database::connect(&backup).await.unwrap();
    for (_, id, original) in &originals {
        assert_eq!(body(&connection, id).await, *original);
    }
    let has_assets: bool = connection
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='agent_blobs')")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!has_assets);
    drop(connection);
    drop(backup);
    fixture.database.close().await.unwrap();
    let database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
    let fixture = fixture::Fixture {
        database,
        ..fixture
    };
    assert_eq!(fixture.read(None, 100).await, projected);
    assert_eq!(
        std::fs::read_dir(fixture._root.path().join("backups"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn rejects_invalid_history_without_writes() {
    let mut fixture = fixture::Fixture::new().await;
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let asset = super::super::blobs::put(&transaction, "image/png", &[1, 2, 3])
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let first = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Image { asset }],
    );
    let second = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Text {
            text: "Later record".into(),
        }],
    );
    let turn = fixture.append([first.clone(), second.clone()]).await;
    let first_body=serde_json::to_vec_pretty(&json!({"id":first.id,"timestamp":first.created,"author":"assistant","branch":"","turn_complete":false,"interrupted":false,"content":{"role":"model","parts":[{"mime_type":"image/png","data":[1,2,3]}]},"actions":{"state_delta":{},"artifact_delta":{}}})).unwrap();
    let invalid = br#"{"id":"second","partial":true}"#.to_vec();
    let originals = vec![
        (turn, first.id.clone(), first_body),
        (turn, second.id.clone(), invalid),
    ];
    legacy(&fixture.database.connection, &originals).await;
    let path = fixture.database.path.clone();
    let root = fixture._root.path().to_owned();
    fixture.database.close().await.unwrap();
    let error = match Database::open(&path, NodeId([33; 32]), None).await {
        Ok(_) => panic!("invalid history was accepted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("cannot be upgraded (decode:"), "{error}");
    assert!(!root.join("backups").exists());
    let handle = crate::store::database::engine(&path).await.unwrap();
    let connection = crate::store::database::connect(&handle).await.unwrap();
    for (_, id, original) in &originals {
        assert_eq!(body(&connection, id).await, *original);
    }
    let assets: bool = connection
        .prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='agent_blobs')")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert!(!assets);
}
