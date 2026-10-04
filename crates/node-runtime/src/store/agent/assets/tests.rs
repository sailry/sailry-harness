use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

fn query() -> Query {
    Query {
        kind: None,
        before: None,
        limit: 20,
    }
}
fn page(fixture: &Fixture, query: Query) -> assets::Page {
    read(&fixture.database.connection, fixture.session, &query).unwrap()
}

#[test]
fn extracts_explicit_files_and_deduplicates() {
    let mut fixture = Fixture::new();
    fixture.completed(&["[Report](report.md) ![Image](image.png) [Again](./report.md#L3) `![Code](code.png)` [Outside](../secret) [Web](https://example.test/a.png) plain.txt"]);
    let result = page(&fixture, query());
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
        .groups
        .is_empty()
    );
}

#[test]
fn includes_inputs_and_inline_images() {
    let mut fixture = Fixture::new();
    let mut user = AdkEvent::new("asset-input");
    user.author = "user".into();
    user.set_content(adk_core::Content::new("user").with_text("[Input](input.txt)"));
    let mut assistant = AdkEvent::new("asset-image");
    assistant.author = "assistant".into();
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let mut content = adk_core::Content::new("model");
    content.parts.push(adk_core::Part::InlineData {
        mime_type: "image/png".into(),
        data: image.into_inner(),
        uri: None,
        annotations: None,
    });
    assistant.set_content(content);
    fixture.append([user, assistant]);
    let result = page(&fixture, query());
    assert_eq!(result.groups.len(), 2);
    assert_eq!(result.groups[0].kind, Kind::Artifact);
    assert!(matches!(result.groups[0].items[0], Target::Image(_)));
    assert_eq!(result.groups[1].kind, Kind::Resource);
    assert_eq!(
        page(
            &fixture,
            Query {
                kind: Some(Kind::Resource),
                ..query()
            }
        )
        .groups
        .len(),
        1
    );
}

#[test]
fn pages_sparse_history_and_preserves_fork_scope() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["[Old](old.md)"]);
    let Output::Session(fork) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: first,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected")
    };
    fixture.completed(&vec!["No resources here"; 300]);
    fixture.completed(&["[New](new.md)"]);
    let result = page(&fixture, query());
    assert_eq!(result.groups.len(), 1);
    let next = page(
        &fixture,
        Query {
            before: result.next_before,
            ..query()
        },
    );
    assert_eq!(next.groups[0].turn, first);
    assert!(next.next_before.is_none());
    let inherited = read(&fixture.database.connection, fork.id, &query()).unwrap();
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
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
}

#[test]
fn validates_bounds_and_uses_read_only_dispatch() {
    let mut fixture = Fixture::new();
    fixture.completed(&["[File](file.txt)"]);
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
        command(&mut fixture.database, &fixture.events, command_value),
        Output::ConversationAssets(_)
    ));
}

#[test]
fn preserves_literal_reference_and_written_paths() {
    let mut fixture = Fixture::new();
    fixture.completed(&["fixture"]);
    let mut entry = fixture.read(None, 1).page.entries[0].clone();
    let path = "notes # 100%.md";
    entry.parts = vec![
        Part::Reference(reference::Reference {
            target: reference::Target::File(path.into()),
            label: path.into(),
        }),
        Part::ToolResult {
            id: None,
            name: crate::plugins::tools::alias("files", "write_file"),
            result: serde_json::to_value(Output::FileWritten(FileWritten {
                path: path.into(),
                revision: "written".into(),
                size: 7,
            }))
            .unwrap(),
            images: vec![],
        },
        Part::ToolResult {
            id: None,
            name: crate::plugins::tools::alias("files", "write_file"),
            result: serde_json::json!({"error": "failed", "path": "missing.txt"}),
            images: vec![],
        },
    ];
    assert_eq!(
        extract::items(&entry, Some("/project")),
        [Target::File { path: path.into() }]
    );
}

#[test]
fn indexes_office_outputs() {
    let mut fixture = Fixture::new();
    fixture.completed(&["fixture"]);
    let mut entry = fixture.read(None, 1).page.entries[0].clone();
    let file = FileWritten {
        path: "report.docx".into(),
        revision: "written".into(),
        size: 1024,
    };
    {
        let (name, output) = (
            crate::plugins::tools::alias("files", "export_pdf"),
            Output::OfficeWritten(sailry_protocol::office::Written {
                file: file.clone(),
                warnings: vec!["Font substituted".into()],
            }),
        );
        entry.parts = vec![Part::ToolResult {
            id: None,
            name,
            result: serde_json::to_value(output).unwrap(),
            images: vec![],
        }];
        assert_eq!(
            extract::items(&entry, Some("/project")),
            [Target::File {
                path: file.path.clone()
            }]
        );
    }
}

#[test]
fn indexes_valid_plugin_files() {
    let mut fixture = Fixture::new();
    fixture.completed(&["fixture"]);
    let mut entry = fixture.read(None, 1).page.entries[0].clone();
    let content = serde_json::json!({
        "sailry_content": {"version": 1, "blocks": [
            {"kind": "file", "path": "output/report.pdf", "mime": "application/pdf"}
        ]}
    });
    for result in [content.clone(), serde_json::json!({"output": content})] {
        entry.parts = vec![Part::ToolResult {
            id: None,
            name: "document_export".into(),
            result,
            images: vec![],
        }];
        assert_eq!(
            extract::items(&entry, Some("/project")),
            [Target::File {
                path: "output/report.pdf".into()
            }]
        );
    }
    if let Part::ToolResult { result, .. } = &mut entry.parts[0] {
        result["output"]["sailry_content"]["blocks"][0]["path"] = "../private.pdf".into();
    }
    assert!(extract::items(&entry, Some("/project")).is_empty());
}
