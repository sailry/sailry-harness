use super::*;

#[tokio::test]
async fn pages_granted_external_sources_with_current_revisions() {
    let fixture = Fixture::start().await;
    let client = fixture.clients().into_iter().next().unwrap();
    let reader = install(
        &client,
        &fixture.root,
        fixture.worktree,
        "source-reader",
        0,
        &[Action::ReadKnowledge],
    )
    .await;
    let external = fixture.temp.path().join("external");
    fs::create_dir(&external).unwrap();
    let path = external.join("source.md");
    let original = (0..80)
        .map(|line| {
            if line == 35 {
                "needleword\n".to_owned()
            } else {
                format!(
                    "Line {line}: supporting source material and 中文上下文。{}\n",
                    "more detail ".repeat(8)
                )
            }
        })
        .collect::<String>();
    fs::write(&path, &original).unwrap();
    let mut definition = definition(Source::Directory {
        path: external.to_str().unwrap().into(),
    });
    definition.shared_with = vec![reader.package.name.clone()];
    let collection = super::saved(
        execute(
            &client,
            Command::CreateKnowledgeCollection {
                definition: definition.clone(),
                plugin: None,
            },
        )
        .await,
    );
    let citation = found(&client, Some(&reader), "needleword", collection.id)
        .await
        .hits
        .remove(0)
        .citation;
    assert_eq!(
        citation.revision,
        blake3::hash(original.as_bytes()).to_hex().to_string()
    );
    let input = CitationRequest {
        collection: collection.id,
        chunk: citation.chunk.clone(),
        revision: citation.revision.clone(),
        context: true,
        offset: 0,
        limit: 97,
    };
    for client in fixture.clients() {
        let mut request = input.clone();
        let mut text = String::new();
        loop {
            let Output::KnowledgeCitation(page) = run(
                &client,
                &reader,
                Command::ReadKnowledgeCitation(request.clone()),
            )
            .await
            else {
                panic!("citation expected")
            };
            assert!(page.untrusted && page.context);
            assert_eq!(page.citation, citation);
            assert_eq!(page.offset, request.offset);
            assert_eq!(page.worktree, None);
            assert!(page.text.len() <= request.limit as usize);
            text.push_str(&page.text);
            let Some(offset) = page.next_offset else {
                break;
            };
            assert!(offset > request.offset);
            assert!(original.is_char_boundary(offset as usize));
            request.offset = offset;
        }
        assert_eq!(text, original);
        let mut selected = input.clone();
        selected.context = false;
        selected.limit = 64 * 1024;
        let Output::KnowledgeCitation(excerpt) =
            run(&client, &reader, Command::ReadKnowledgeCitation(selected)).await
        else {
            panic!("citation expected")
        };
        assert!(!excerpt.context && excerpt.untrusted);
        assert!(excerpt.text.contains("needleword"));
        assert!(excerpt.text.len() < original.len());
    }
    definition.shared_with.clear();
    let updated = super::saved(
        execute(
            &client,
            Command::UpdateKnowledgeCollection {
                id: collection.id,
                expected_revision: collection.revision,
                definition,
            },
        )
        .await,
    );
    for client in fixture.clients() {
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadKnowledgeCitation(input.clone()))
                        .with_plugin(reader.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    fs::write(&path, "Changed source\n").unwrap();
    assert_eq!(
        client
            .execute(client.prepare(Command::ReadKnowledgeCitation(input)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    execute(
        &client,
        Command::DeleteKnowledgeCollection {
            id: collection.id,
            expected_revision: updated.revision,
        },
    )
    .await;
    fixture.close().await;
}
