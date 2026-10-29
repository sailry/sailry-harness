use super::*;
use crate::{
    agent::canonical::{Attachment as StoredAttachment, BlockKind, Record, Role},
    store::agent::blobs,
};
use fixture::{message, response};

#[tokio::test]
async fn preserves_large_images() {
    let mut fixture = fixture::Fixture::new().await;
    let bytes = vec![255; 600 * 1024];
    assert!(serde_json::to_vec(&bytes).unwrap().len() > 2 * 1024 * 1024);
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let asset = blobs::put(&transaction, "image/png", &bytes).await.unwrap();
    transaction.commit().await.unwrap();
    let mut result = response(
        "capture",
        "capture",
        serde_json::json!({"image_width":5120}),
    );
    result.attachments.push(StoredAttachment::Inline { asset });
    let event = message(
        "assistant",
        Role::User,
        vec![BlockKind::ToolResponse(result)],
    );
    fixture.append([event]).await;
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events ORDER BY sequence DESC LIMIT 1")
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(body.len() < 4096);
    let restored: Record = serde_json::from_slice(&body).unwrap();
    let BlockKind::ToolResponse(response) = &restored.message().unwrap().content[0].kind else {
        panic!("capture result expected")
    };
    let StoredAttachment::Inline { asset } = &response.attachments[0];
    assert_eq!(
        super::super::blobs::get(&fixture.database.connection, &asset.digest)
            .await
            .unwrap(),
        bytes
    );
    let projected = fixture.read(None, 100).await.page.entries;
    let Part::ToolResult { images, .. } = &projected[0].parts[0] else {
        panic!("tool result expected")
    };
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].attachment.spec.name, "image-1.png");
    assert_eq!(images[0].attachment.spec.size, bytes.len() as u64);
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &images[0])
            .await
            .unwrap(),
        bytes
    );
    assert!(encode(&projected).unwrap().len() < 4096);
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page.entries, projected);
}

#[tokio::test]
async fn projects_json_media() {
    use crate::agent::{canonical::ToolFormat, engine::history as conversion};
    use goose_provider_types::conversation::message::{Message, MessageContent};
    use rmcp::model::{CallToolResult, ContentBlock};
    use std::collections::BTreeMap;

    for computer in [false, true] {
        let mut fixture = fixture::Fixture::new().await;
        let mut value = serde_json::json!({
            "response":{"text":"Exact capture metadata"},
            "inline_data":[
                {"mime_type":"audio/wav","data":[4,5,6]},
                {"mime_type":"image/png","data":[1,2,3]}
            ],
            "business":{"mime_type":"image/jpeg","data":[7,8,9]}
        });
        if computer {
            value = serde_json::json!({"kind":"computer","data":value});
        }
        let source = Message::user()
            .with_generated_id()
            .with_tool_response("capture", Ok(CallToolResult::structured(value.clone())));
        let record = conversion::capture(
            &fixture.database.connection,
            TurnId::new(),
            &source,
            &BTreeMap::from([("capture".into(), "capture".into())]),
        )
        .await
        .unwrap();
        let id = record.id.clone();
        fixture.append([record]).await;
        let body: Vec<u8> = fixture
            .database
            .connection
            .prepare("SELECT body FROM agent_events WHERE id=?1")
            .await
            .unwrap()
            .query_row([id])
            .await
            .unwrap()
            .get(0)
            .unwrap();
        let record: Record = serde_json::from_slice(&body).unwrap();
        let BlockKind::ToolResponse(response) = &record.message().unwrap().content[0].kind else {
            panic!("captured tool result expected")
        };
        assert_eq!(response.format, ToolFormat::Json);
        assert!(response.attachments.is_empty());
        assert_eq!(response.result.assets.len(), 3);
        assert!(response.result.assets.iter().all(|slot| {
            response.result.value.pointer(&slot.pointer) == Some(&serde_json::Value::Null)
        }));
        assert_eq!(
            response.revision,
            blake3::hash(value.to_string().as_bytes())
                .to_hex()
                .to_string()
        );
        assert_eq!(
            blobs::hydrate(&fixture.database.connection, &response.result)
                .await
                .unwrap(),
            value
        );
        let entries = fixture.read(None, 100).await.page.entries;
        let Part::ToolResult { images, .. } = &entries[0].parts[0] else {
            panic!("public tool result expected")
        };
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].index, 1);
        assert_eq!(images[0].attachment.spec.media_type, "image/png");
        assert_eq!(
            super::super::images::resolve(
                &fixture.database.connection,
                fixture.session,
                &images[0]
            )
            .await
            .unwrap(),
            [1, 2, 3]
        );
        let mut audio = images[0].clone();
        audio.index = 0;
        assert_eq!(
            super::super::images::resolve(&fixture.database.connection, fixture.session, &audio)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let projected = conversion::project(&fixture.database.connection, &record)
            .await
            .unwrap()
            .unwrap();
        let MessageContent::ToolResponse(response) = &projected.content[0] else {
            panic!("outgoing tool result expected")
        };
        let result = response.tool_result.as_ref().unwrap();
        assert_eq!(
            result
                .content
                .iter()
                .filter(|part| matches!(part, ContentBlock::Image(_)))
                .count(),
            1
        );
        assert_eq!(
            result
                .content
                .iter()
                .filter(|part| matches!(part, ContentBlock::Audio(_)))
                .count(),
            1
        );
        history::rebuild(&mut fixture.database.connection)
            .await
            .unwrap();
        assert_eq!(fixture.read(None, 100).await.page.entries, entries);
        assert_eq!(
            super::super::images::resolve(
                &fixture.database.connection,
                fixture.session,
                &images[0]
            )
            .await
            .unwrap(),
            [1, 2, 3]
        );
    }
}

#[tokio::test]
async fn validates_source_metadata() {
    let mut fixture = fixture::Fixture::new().await;
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let mut result = response(
        "image-tool",
        "any_image_tool",
        serde_json::json!({"ok":true}),
    );
    for mime in ["audio/wav", "image/png", "image/jpeg"] {
        result.attachments.push(StoredAttachment::Inline {
            asset: blobs::put(&transaction, mime, &[1, 2, 3]).await.unwrap(),
        });
    }
    transaction.commit().await.unwrap();
    let event = message(
        "",
        Role::User,
        vec![
            BlockKind::Text {
                text: "Image results".into(),
            },
            BlockKind::ToolResponse(result),
        ],
    );
    fixture.append([event]).await;
    let page = fixture.read(None, 100).await.page;
    let Part::ToolResult { images, .. } = &page.entries[0].parts[1] else {
        panic!("tool result expected")
    };
    assert_eq!(
        images
            .iter()
            .map(|image| (image.part, image.index))
            .collect::<Vec<_>>(),
        [(1, 1), (1, 2)]
    );
    assert_ne!(images[0].attachment.id, images[1].attachment.id);
    assert_eq!(images[0].attachment.spec.media_type, "image/png");
    assert_eq!(images[1].attachment.spec.media_type, "image/jpeg");
    for index in 0..6 {
        let mut altered = images[0].clone();
        match index {
            0 => altered.attachment.id = AttachmentId::new(),
            1 => altered.attachment.spec.worktree = WorktreeId::new(),
            2 => altered.attachment.spec.name = "different.png".into(),
            3 => altered.attachment.spec.media_type = "image/gif".into(),
            4 => altered.attachment.spec.size += 1,
            _ => altered.attachment.spec.revision = "different".into(),
        }
        assert_eq!(
            super::super::images::resolve(&fixture.database.connection, fixture.session, &altered)
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    for (part, index) in [(0, 1), (1, 0), (1, 3)] {
        let altered = Image {
            part,
            index,
            ..images[0].clone()
        };
        assert_eq!(
            super::super::images::resolve(&fixture.database.connection, fixture.session, &altered)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
}

#[tokio::test]
async fn preserves_assistant_parts() {
    let mut fixture = fixture::Fixture::new().await;
    let bytes = vec![255; 600 * 1024];
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let asset = blobs::put(&transaction, "image/png", &bytes).await.unwrap();
    transaction.commit().await.unwrap();
    let event = message(
        "assistant",
        Role::Assistant,
        vec![
            BlockKind::Text {
                text: "Before the image".into(),
            },
            BlockKind::Image { asset },
            BlockKind::Text {
                text: "After the image".into(),
            },
        ],
    );
    fixture.append([event]).await;
    let entries = fixture.read(None, 100).await.page.entries;
    let parts = &entries[0].parts;
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], Part::Text("Before the image".into()));
    assert_eq!(parts[2], Part::Text("After the image".into()));
    let Part::Image(image) = &parts[1] else {
        panic!("assistant image expected")
    };
    assert_eq!((image.part, image.index), (1, 0));
    assert_eq!(image.attachment.spec.size, bytes.len() as u64);
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, image)
            .await
            .unwrap(),
        bytes
    );
    assert!(encode(&entries).unwrap().len() < 4096);
    let mut altered = image.clone();
    altered.index = 1;
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &altered)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(fixture.read(None, 100).await.page.entries, entries);
}

#[tokio::test]
async fn resolves_visible_history() {
    let mut fixture = fixture::Fixture::new().await;
    let before = fixture.completed(&["Before the image"]).await;
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let asset = blobs::put(&transaction, "image/png", &[1, 2, 3])
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let mut result = response("capture", "capture", serde_json::json!({}));
    result.attachments.push(StoredAttachment::Inline { asset });
    let event = message("", Role::User, vec![BlockKind::ToolResponse(result)]);
    let head = fixture.append([event]).await;
    let page = fixture.read(None, 100).await.page;
    let image = page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .find_map(|part| {
            if let Part::ToolResult { images, .. } = part {
                images.first().cloned()
            } else {
                None
            }
        })
        .unwrap();
    let Output::Session(early) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: before,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, early.id, &image)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fork.id, &image)
            .await
            .unwrap(),
        [1, 2, 3]
    );
    let Output::Rewound(rewound) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: fixture.session,
            through: Some(before),
            expected_head: head,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("rewind expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &image)
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert!(rewound.backup.is_none());
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fork.id, &image)
            .await
            .unwrap(),
        [1, 2, 3]
    );
}

#[tokio::test]
async fn rejects_oversized_events() {
    let mut fixture = fixture::Fixture::new().await;
    let turn = fixture.queued().await;
    let event = Record {
        turn,
        ..message(
            "",
            Role::Assistant,
            vec![BlockKind::Text {
                text: "x".repeat(MAX_FRAME_BYTES),
            }],
        )
    };
    let error = super::super::history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("history frame limit"));
}
