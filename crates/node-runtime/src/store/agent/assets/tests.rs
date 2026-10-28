use super::*;
use crate::{
    agent::canonical::{BlockKind, Record, Role, ToolFormat},
    store::agent::{
        blobs,
        tests::{
            command,
            fixture::{Fixture, message, response},
        },
    },
};

async fn publish(fixture: &Fixture, items: &mut Vec<Target>, results: Vec<serde_json::Value>) {
    let record = message(
        "assistant",
        Role::Assistant,
        results
            .into_iter()
            .map(|value| BlockKind::ToolResponse(response(None, "document_export", value)))
            .collect(),
    );
    extract::publications(&fixture.database.connection, items, &record)
        .await
        .unwrap();
}

fn query() -> Query {
    Query {
        kind: None,
        before: None,
        limit: 20,
    }
}
async fn page(fixture: &Fixture, query: Query) -> assets::Page {
    read(&fixture.database.connection, fixture.session, &query)
        .await
        .unwrap()
}

#[tokio::test]
async fn extracts_explicit_files_and_deduplicates() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["[Report](report.md)\n\n![Image](image.png)\n\n[Again](./report.md)\n\nSource: [Code](src/main.rs#L3) `![Code](code.png)` [Outside](../secret) [Web](https://example.test/a.png) plain.txt"]).await;
    let result = page(&fixture, query()).await;
    assert_eq!(result.groups.len(), 1);
    assert_eq!(
        result.groups[0].items,
        vec![
            Target::File {
                path: "report.md".into()
            },
            Target::File {
                path: "image.png".into()
            }
        ]
    );
    assert_eq!(result.groups[0].kind, Kind::Artifact);
    assert!(
        page(
            &fixture,
            Query {
                kind: Some(Kind::Resource),
                ..query()
            }
        )
        .await
        .groups
        .is_empty()
    );
}

#[tokio::test]
async fn includes_inputs_and_inline_images() {
    let mut fixture = Fixture::new().await;
    let user = message(
        "user",
        Role::User,
        vec![BlockKind::Text {
            text: "See [Input](input.txt)\n\n> [Quoted](quoted.txt)".into(),
        }],
    );
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let transaction = fixture
        .database
        .connection
        .unchecked_transaction()
        .await
        .unwrap();
    let asset = blobs::put(&transaction, "image/png", &image.into_inner())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let assistant = message(
        "assistant",
        Role::Assistant,
        vec![BlockKind::Image { asset }],
    );
    fixture.append([user, assistant]).await;
    let result = page(&fixture, query()).await;
    assert_eq!(result.groups.len(), 2);
    assert_eq!(result.groups[0].kind, Kind::Artifact);
    assert!(matches!(result.groups[0].items[0], Target::Image(_)));
    assert_eq!(result.groups[1].kind, Kind::Resource);
    assert_eq!(
        result.groups[1].items,
        [
            Target::File {
                path: "input.txt".into()
            },
            Target::File {
                path: "quoted.txt".into()
            },
        ]
    );
    assert_eq!(
        page(
            &fixture,
            Query {
                kind: Some(Kind::Resource),
                ..query()
            }
        )
        .await
        .groups
        .len(),
        1
    );
}

#[tokio::test]
async fn pages_sparse_history_and_preserves_fork_scope() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&["[Old](old.md)"]).await;
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: first,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    fixture.completed(&vec!["No resources here"; 300]).await;
    fixture.completed(&["[New](new.md)"]).await;
    let result = page(&fixture, query()).await;
    assert_eq!(result.groups.len(), 1);
    let next = page(
        &fixture,
        Query {
            before: result.next_before,
            ..query()
        },
    )
    .await;
    assert_eq!(next.groups[0].turn, first);
    assert!(next.next_before.is_none());
    let inherited = read(&fixture.database.connection, fork.id, &query())
        .await
        .unwrap();
    assert_eq!(inherited.groups.len(), 1);
    assert_eq!(inherited.groups[0].turn, first);
    assert_eq!(
        read(
            &fixture.database.connection,
            fork.id,
            &Query {
                before: Some(result.groups[0].sequence),
                ..query()
            }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
}

#[tokio::test]
async fn validates_bounds_and_uses_read_only_dispatch() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["[File](file.txt)"]).await;
    for invalid_query in [
        Query {
            limit: 0,
            ..query()
        },
        Query {
            limit: assets::MAX_GROUPS + 1,
            ..query()
        },
        Query {
            before: Some(0),
            ..query()
        },
    ] {
        assert_eq!(
            read(
                &fixture.database.connection,
                fixture.session,
                &invalid_query
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    let command_value = Command::ListConversationAssets {
        session: fixture.session,
        query: query(),
    };
    assert!(!command_value.durable());
    assert!(matches!(
        command(&mut fixture.database, &fixture.events, command_value).await,
        Output::ConversationAssets(_)
    ));
}

#[tokio::test]
async fn preserves_literal_references_without_indexing_writes() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["fixture"]).await;
    let mut entry = fixture.read(None, 1).await.page.entries[0].clone();
    let path = "notes # 100%.md";
    entry.parts = vec![
        Part::Reference(reference::Reference {
            target: reference::Target::File(path.into()),
            label: path.into(),
        }),
        Part::ToolResult {
            id: None,
            name: crate::plugins::tools::alias("files", "write_file"),
            summary: Default::default(),
            images: vec![],
        },
        Part::ToolResult {
            id: None,
            name: crate::plugins::tools::alias("files", "write_file"),
            summary: Default::default(),
            images: vec![],
        },
    ];
    let mut items = extract::items(&entry, Some("/project"));
    publish(
        &fixture,
        &mut items,
        vec![
            serde_json::to_value(Output::FileWritten(FileWritten {
                path: path.into(),
                revision: "written".into(),
                size: 7,
            }))
            .unwrap(),
            serde_json::json!({"error":"failed","path":"missing.txt"}),
        ],
    )
    .await;
    assert_eq!(items, [Target::File { path: path.into() }]);
}

#[tokio::test]
async fn project_writes_are_not_outputs() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["fixture"]).await;
    let mut entry = fixture.read(None, 1).await.page.entries[0].clone();
    entry.parts = (0..3_000)
        .map(|_| Part::ToolResult {
            id: None,
            name: crate::plugins::tools::alias("files", "write_file"),
            summary: Default::default(),
            images: vec![],
        })
        .collect();
    assert!(extract::items(&entry, Some("/project")).is_empty());
    let mut items = Vec::new();
    publish(
        &fixture,
        &mut items,
        (0..3_000)
            .map(|index| {
                serde_json::to_value(Output::FileWritten(FileWritten {
                    path: format!("src/module_{index}.rs"),
                    revision: "written".into(),
                    size: 7,
                }))
                .unwrap()
            })
            .collect(),
    )
    .await;
    assert!(items.is_empty());
    entry.parts.push(Part::Text("Changed [Source](src/main.rs).\n\n[Data](output/data.json)\n\n[Script](output/stats.py)\n\n[Log](output/execution.log)".into()));
    assert_eq!(
        extract::items(&entry, Some("/project")),
        [
            Target::File {
                path: "output/data.json".into()
            },
            Target::File {
                path: "output/stats.py".into()
            },
            Target::File {
                path: "output/execution.log".into()
            },
        ]
    );
}

#[tokio::test]
async fn distinguishes_deliveries_from_navigation() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["[Line](src/main.rs:42)\n\n[Section](guide.md#intro)\n\n[One](a.json) [Two](b.json)\n\nSee [Report](report.pdf)\n\n> [Source](src/quoted.rs)\n\n> > [Nested](src/nested.rs)\n\n`[Code](code.py)`\n\n[Web](https://example.test) [Mixed](mixed.json)\n\n- [Report](report.pdf)\n- [Data](data.json)"]).await;
    let result = page(&fixture, query()).await;
    assert_eq!(
        result.groups[0].items,
        [
            Target::File {
                path: "report.pdf".into()
            },
            Target::File {
                path: "data.json".into()
            },
        ]
    );
}

#[tokio::test]
async fn ignores_document_write_receipts() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["fixture"]).await;
    let mut entry = fixture.read(None, 1).await.page.entries[0].clone();
    let file = FileWritten {
        path: "report.docx".into(),
        revision: "written".into(),
        size: 1024,
    };
    {
        let (name, output) = (
            crate::plugins::tools::alias("files", "write_file"),
            Output::FileWritten(file.clone()),
        );
        entry.parts = vec![Part::ToolResult {
            id: None,
            name,
            summary: Default::default(),
            images: vec![],
        }];
        assert!(extract::items(&entry, Some("/project")).is_empty());
        let mut items = Vec::new();
        publish(
            &fixture,
            &mut items,
            vec![serde_json::to_value(output).unwrap()],
        )
        .await;
        assert!(items.is_empty());
    }
}

#[tokio::test]
async fn indexes_valid_plugin_files() {
    let mut fixture = Fixture::new().await;
    fixture.completed(&["fixture"]).await;
    let entry = fixture.read(None, 1).await.page.entries[0].clone();
    let content = serde_json::json!({
        "sailry_content": {"version": 1, "blocks": [
            {"kind": "file", "path": "output/report.pdf", "mime": "application/pdf"}
        ]}
    });
    for (result, format) in [
        (content.clone(), ToolFormat::Json),
        (
            serde_json::json!({"output": content.clone()}),
            ToolFormat::Json,
        ),
        (
            serde_json::json!({"structuredContent":content,"content":[]}),
            ToolFormat::Mcp,
        ),
    ] {
        let mut items = extract::items(&entry, Some("/project"));
        let mut output = response(None, "document_export", result.clone());
        output.format = format;
        let record = message(
            "assistant",
            Role::Assistant,
            vec![BlockKind::ToolResponse(output)],
        );
        extract::publications(&fixture.database.connection, &mut items, &record)
            .await
            .unwrap();
        let BlockKind::ToolResponse(output) = &record.message().unwrap().content[0].kind else {
            panic!("tool response expected");
        };
        assert_eq!(
            blobs::hydrate(&fixture.database.connection, &output.result)
                .await
                .unwrap(),
            result
        );
        assert_eq!(
            items,
            [Target::File {
                path: "output/report.pdf".into()
            }]
        );
    }
    let mut items = Vec::new();
    publish(
        &fixture,
        &mut items,
        vec![serde_json::json!({
            "sailry_content":{"version":1,"blocks":[{"kind":"file","path":"../private.pdf","mime":"application/pdf"}]}
        })],
    ).await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn pages_document_receipts_with_fork_scope() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let document = sailry_protocol::artifact::Summary {
        kind: sailry_protocol::artifact::Kind::Document,
        id: ArtifactId::new(),
        session: fixture.session,
        turn,
        package: sailry_protocol::plugin::Reference {
            name: "document-fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        title: "HTML report".into(),
        summary: "Captured metadata".into(),
        revision: 1,
        size: 16,
        created_at: 0,
        updated_at: 1,
    };
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let receipt = sailry_protocol::tool::Content {
        version: 1,
        blocks: vec![
            sailry_protocol::tool::Block::Artifact {
                artifact: document.clone(),
            },
            sailry_protocol::tool::Block::Artifact {
                artifact: document.clone(),
            },
        ],
    };
    let event = Record {
        turn,
        ..message(
            "assistant",
            Role::User,
            vec![BlockKind::ToolResponse(response(
                Some("document-write"),
                "document_write",
                serde_json::json!({"sailry_content":receipt}),
            ))],
        )
    };
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: turn,
            expected_revision: 1,
        },
    )
    .await
    else {
        panic!("fork expected")
    };
    fixture
        .completed(&vec!["Later conversation without outputs"; 300])
        .await;
    fixture.completed(&["[New file](new.txt)"]).await;
    assert!(
        !fixture
            .read(None, 1)
            .await
            .page
            .runs
            .iter()
            .any(|run| run.turn == turn)
    );
    let mut scan = Query {
        limit: 1,
        ..query()
    };
    let mut found = Vec::new();
    loop {
        let result = page(&fixture, scan.clone()).await;
        found.extend(result.groups);
        let Some(before) = result.next_before else {
            break;
        };
        scan.before = Some(before);
    }
    let receipt = found.iter().find(|group| group.turn == turn).unwrap();
    assert_eq!(receipt.kind, Kind::Artifact);
    assert_eq!(receipt.items, [Target::Document(document.clone())]);
    let inherited = read(&fixture.database.connection, branch.id, &query())
        .await
        .unwrap();
    assert_eq!(inherited.groups.len(), 1);
    assert_eq!(inherited.groups[0].items, [Target::Document(document)]);
    assert!(
        read(&fixture.database.connection, SessionId::new(), &query())
            .await
            .is_err()
    );
}
