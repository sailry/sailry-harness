use super::*;

#[test]
fn preserves_large_images() {
    let mut fixture = fixture::Fixture::new();
    let bytes = vec![255; 600 * 1024];
    let mut event = AdkEvent::new("driver-image-fixture");
    event.author = "assistant".into();
    let mut content = adk_core::Content::new("tool");
    content.parts.push(adk_core::Part::FunctionResponse {
        id: Some("capture".into()),
        annotations: None,
        function_response: adk_core::FunctionResponseData::with_inline_data(
            "capture",
            serde_json::json!({"image_width":5120}),
            vec![adk_core::InlineDataPart {
                mime_type: "image/png".into(),
                data: bytes.clone(),
                uri: None,
                annotations: None,
            }],
        ),
    });
    event.set_content(content);
    assert!(encode(&event).unwrap().len() > 2 * 1024 * 1024);
    fixture.append([event]);
    let body: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT body FROM agent_events ORDER BY sequence DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let restored: AdkEvent = serde_json::from_slice(&body).unwrap();
    let adk_core::Part::FunctionResponse {
        function_response, ..
    } = &restored.content().unwrap().parts[0]
    else {
        panic!("capture result expected")
    };
    assert_eq!(function_response.inline_data[0].data, bytes);
    let projected = fixture.read(None, 100).page.entries;
    let Part::ToolResult { images, .. } = &projected[0].parts[0] else {
        panic!("tool result expected")
    };
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].attachment.spec.name, "image-1.png");
    assert_eq!(images[0].attachment.spec.size, bytes.len() as u64);
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &images[0])
            .unwrap(),
        bytes
    );
    assert!(encode(&projected).unwrap().len() < 4096);
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(fixture.read(None, 100).page.entries, projected);
}

#[test]
fn validates_source_metadata() {
    let mut fixture = fixture::Fixture::new();
    let mut event = AdkEvent::new("mixed-media");
    let mut content = adk_core::Content::new("tool").with_text("Image results");
    content.parts.push(adk_core::Part::FunctionResponse {
        id: Some("image-tool".into()),
        annotations: None,
        function_response: adk_core::FunctionResponseData::with_inline_data(
            "any_image_tool",
            serde_json::json!({"ok":true}),
            ["audio/wav", "image/png", "image/jpeg"]
                .map(|mime_type| adk_core::InlineDataPart {
                    mime_type: mime_type.into(),
                    data: vec![1, 2, 3],
                    uri: None,
                    annotations: None,
                })
                .to_vec(),
        ),
    });
    event.set_content(content);
    fixture.append([event]);
    let page = fixture.read(None, 100).page;
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
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
}

#[test]
fn preserves_assistant_parts() {
    let mut fixture = fixture::Fixture::new();
    let bytes = vec![255; 600 * 1024];
    let mut event = AdkEvent::new("assistant-image");
    event.author = "assistant".into();
    let mut content = adk_core::Content::new("model").with_text("Before the image");
    content.parts.push(adk_core::Part::InlineData {
        mime_type: "image/png".into(),
        data: bytes.clone(),
        uri: None,
        annotations: None,
    });
    content = content.with_text("After the image");
    event.set_content(content);
    fixture.append([event]);
    let entries = fixture.read(None, 100).page.entries;
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
            .unwrap(),
        bytes
    );
    assert!(encode(&entries).unwrap().len() < 4096);
    let mut altered = image.clone();
    altered.index = 1;
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &altered)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(fixture.read(None, 100).page.entries, entries);
}

#[test]
fn resolves_visible_history() {
    let mut fixture = fixture::Fixture::new();
    let before = fixture.completed(&["Before the image"]);
    let mut event = AdkEvent::new("visible-image");
    event.set_content(adk_core::Content {
        role: "tool".into(),
        parts: vec![adk_core::Part::FunctionResponse {
            id: Some("capture".into()),
            annotations: None,
            function_response: adk_core::FunctionResponseData::with_inline_data(
                "capture",
                serde_json::json!({}),
                vec![adk_core::InlineDataPart {
                    mime_type: "image/png".into(),
                    data: vec![1, 2, 3],
                    uri: None,
                    annotations: None,
                }],
            ),
        }],
    });
    let head = fixture.append([event]);
    let page = fixture.read(None, 100).page;
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
    ) else {
        panic!("fork expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, early.id, &image)
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
    ) else {
        panic!("fork expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fork.id, &image).unwrap(),
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
    ) else {
        panic!("rewind expected")
    };
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, fixture.session, &image)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        super::super::images::resolve(&fixture.database.connection, rewound.backup.id, &image)
            .unwrap(),
        [1, 2, 3]
    );
}

#[test]
fn rejects_oversized_events() {
    let mut fixture = fixture::Fixture::new();
    let turn = fixture.queued();
    let mut event = AdkEvent::new("oversized-fixture");
    event.set_content(adk_core::Content::new("model").with_text("x".repeat(MAX_FRAME_BYTES)));
    let error = super::super::history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .unwrap_err();
    assert!(error.to_string().contains("history frame limit"));
}
