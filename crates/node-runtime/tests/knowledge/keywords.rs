use super::*;

#[tokio::test]
async fn ranks_natural_language_on_both_paths() {
    let fixture = Fixture::start().await;
    for (name, text) in [
        (
            "complete.md",
            "Rust handles cancellation of running async operations\n管理资料，使用关键词完成检索，也提供有效方法\n",
        ),
        ("partial.md", "Rust language syntax\n资料归档\n"),
        ("unrelated.md", "Database pool limits\n其他内容\n"),
    ] {
        fs::write(fixture.root.join(name), text).unwrap();
    }
    let client = fixture.clients().into_iter().next().unwrap();
    let collection = super::saved(
        execute(
            &client,
            Command::CreateKnowledgeCollection {
                definition: definition(Source::Worktree {
                    worktree: fixture.worktree,
                    path: String::new(),
                }),
                plugin: None,
            },
        )
        .await,
    );
    found(&client, None, "Rust", collection.id).await;
    for client in fixture.clients() {
        for text in [
            "rust async cancellation",
            "cancellation rust async",
            "资料检索方法",
            "方法检索资料",
        ] {
            let page = search(&client, None, query(text, collection.id)).await;
            assert!(!page.indexing);
            assert_eq!(page.mode, Mode::Keyword);
            assert_eq!(page.hits.len(), 2);
            assert_eq!(page.hits[0].citation.path, "complete.md");
            assert_eq!(page.hits[1].citation.path, "partial.md");
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn retrieves_gbk_text_and_original_citations() {
    let fixture = Fixture::start().await;
    let original = "# Reference\n管理资料，使用关键词完成检索，也提供有效方法\n";
    let (bytes, _, errors) = encoding_rs::GB18030.encode(original);
    assert!(!errors);
    fs::write(fixture.root.join("reference.txt"), &bytes).unwrap();
    let client = fixture.clients().into_iter().next().unwrap();
    let collection = super::saved(
        execute(
            &client,
            Command::CreateKnowledgeCollection {
                definition: definition(Source::Worktree {
                    worktree: fixture.worktree,
                    path: "reference.txt".into(),
                }),
                plugin: None,
            },
        )
        .await,
    );
    found(&client, None, "资料", collection.id).await;
    for client in fixture.clients() {
        let page = search(&client, None, query("资料检索方法", collection.id)).await;
        assert!(!page.indexing);
        assert_eq!(page.hits.len(), 1);
        let hit = &page.hits[0];
        assert_eq!(
            hit.citation.revision,
            blake3::hash(&bytes).to_hex().to_string()
        );
        assert!(hit.text.contains("管理资料"));
        assert_eq!(hit.citation.position, Position::Lines { start: 1, end: 2 });
        let Output::KnowledgeCitation(opened) = execute(
            &client,
            Command::ReadKnowledgeCitation(CitationRequest {
                context: false,
                collection: collection.id,
                chunk: hit.citation.chunk.clone(),
                revision: hit.citation.revision.clone(),
                offset: 0,
                limit: 4096,
            }),
        )
        .await
        else {
            panic!("citation expected")
        };
        assert_eq!(opened.text, original.trim_end_matches('\n'));
        assert_eq!(opened.worktree, Some(fixture.worktree));
    }
    fixture.close().await;
}
