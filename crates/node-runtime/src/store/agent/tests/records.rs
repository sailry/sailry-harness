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

#[tokio::test]
async fn restores_current_history() {
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
    fixture.append([answer.clone()]).await;
    let mut call = message(
        "assistant",
        Role::Assistant,
        vec![
            BlockKind::Text {
                text: "Before the call".into(),
            },
            BlockKind::ToolRequest(request(
                "recorded-call",
                "read_file",
                json!({"path":"recorded.txt"}),
            )),
        ],
    );
    call.provenance.branch = "recorded-scope".into();
    if let BlockKind::ToolRequest(request) = &mut call.message_mut().unwrap().content[1].kind {
        request.provider =
            Some(serde_json::from_value(json!({"thoughtSignature":"recorded-signature"})).unwrap());
    }
    let original = json!({"content":"Recorded tool result","image":{"mimeType":"image/png","data":"AAEC/f7/"}});
    let bytes = vec![0, 1, 2, 253, 254, 255];
    let mut output = response("recorded-call", "read_file", original.clone());
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
    fixture.append([call.clone(), result.clone()]).await;
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
    let mut originals = Vec::new();
    for id in [&answer.id, &call.id, &result.id, &summary.id] {
        originals.push((id.clone(), body(&fixture.database.connection, id).await));
    }
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
    for (id, original) in &originals {
        assert_eq!(body(&fixture.database.connection, id).await, *original);
    }
    assert!(!fixture._root.path().join("backups").exists());
    fixture.database.close().await.unwrap();
    let database = Database::open(&path, NodeId([33; 32]), None).await.unwrap();
    let fixture = fixture::Fixture {
        database,
        ..fixture
    };
    assert_eq!(fixture.read(None, 100).await, projected);
    assert!(!fixture._root.path().join("backups").exists());
}
