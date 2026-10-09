use super::*;

#[tokio::test]
async fn opens_office_positions_on_execution_node() {
    let fixture = Fixture::start().await;
    let external = fixture.temp.path().join("external-office");
    fs::create_dir(&external).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/office/fixtures");
    for name in ["report.pdf", "book.xlsx", "deck.pptx"] {
        fs::copy(fixtures.join(name), external.join(name)).unwrap();
    }
    let other = Fixture::start().await;
    for client in fixture.clients() {
        let collection = super::saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Directory {
                        path: external.to_str().unwrap().into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        for (query, expected) in [
            ("PDF acceptance", Position::Page { page: 1 }),
            (
                "合计",
                Position::Cells {
                    sheet: "预算".into(),
                    start: "A1".into(),
                    end: "B3".into(),
                },
            ),
            ("中文幻灯片", Position::Slide { slide: 1 }),
        ] {
            let page = found(&client, None, query, collection.id).await;
            let hit = &page.hits[0];
            assert_eq!(hit.citation.position, expected);
            assert!(Path::new(&hit.citation.path).starts_with(&external));
            let input = CitationRequest {
                collection: collection.id,
                chunk: hit.citation.chunk.clone(),
                revision: hit.citation.revision.clone(),
                offset: 0,
                limit: 4096,
            };
            let admission = client
                .dispatch(client.prepare(Command::ReadKnowledgeCitation(input.clone())))
                .await
                .unwrap();
            assert!(!admission.receipt.durable);
            let Output::KnowledgeCitation(opened) = admission.completion.await.unwrap().unwrap()
            else {
                panic!("citation expected")
            };
            assert_eq!(opened.citation.position, expected);
            assert!(opened.text.contains(query));
            assert_eq!(opened.worktree, None);
            for foreign in other.clients() {
                assert_eq!(
                    foreign
                        .execute(foreign.prepare(Command::ReadKnowledgeCitation(input.clone())))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
            }
        }
        execute(
            &client,
            Command::DeleteKnowledgeCollection {
                id: collection.id,
                expected_revision: collection.revision,
            },
        )
        .await;
    }
    other.close().await;
    fixture.close().await;
}
