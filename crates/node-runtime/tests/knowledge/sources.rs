use super::*;

#[tokio::test]
async fn captures_worktree_grants_and_original_positions() {
    let fixture = Fixture::start().await;
    fs::create_dir(fixture.root.join("notes")).unwrap();
    fs::write(
        fixture.root.join("notes/reference.md"),
        "# Reference\n我们使用向量数据库\nOriginal final line\n",
    )
    .unwrap();
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let owner = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &format!("documents-{index}"),
            0,
            &[
                Action::ReadFiles,
                Action::ReadKnowledge,
                Action::WriteKnowledge,
            ],
        )
        .await;
        let mut definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: "notes".into(),
        });
        definition.globs = vec!["*.md".into()];
        let collection = super::saved(
            run(
                &client,
                &owner,
                Command::CreateKnowledgeCollection {
                    definition: definition.clone(),
                    plugin: None,
                },
            )
            .await,
        );
        let page = found(&client, Some(&owner), "量数", collection.id).await;
        assert!(page.hits[0].text.contains("我们使用向量数据库"));
        assert!(page.hits[0].citation.path.ends_with("reference.md"));
        assert!(
            matches!(page.hits[0].citation.position,Position::Lines {start,end} if start<=2 && end>=2)
        );
        let mut outside = definition.clone();
        outside.source = Source::Worktree {
            worktree: fixture.worktree,
            path: "../outside".into(),
        };
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CreateKnowledgeCollection {
                            definition: outside,
                            plugin: None
                        })
                        .with_plugin(owner.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut wrong = owner.clone();
        wrong.worktree = None;
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CreateKnowledgeCollection {
                            definition: definition.clone(),
                            plugin: None
                        })
                        .with_plugin(wrong)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for source in [
            Source::File {
                path: fixture
                    .root
                    .join("notes/reference.md")
                    .to_str()
                    .unwrap()
                    .into(),
            },
            Source::Directory {
                path: fixture.root.to_str().unwrap().into(),
            },
        ] {
            assert_eq!(
                client
                    .execute(
                        client
                            .prepare(Command::CreateKnowledgeCollection {
                                definition: super::definition(source),
                                plugin: None
                            })
                            .with_plugin(owner.clone())
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CreateKnowledgeCollection {
                            definition,
                            plugin: Some(owner.package.name.clone())
                        })
                        .with_plugin(owner)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn rejects_symlinks_and_controller_source_mismatches() {
    let fixture = Fixture::start().await;
    let outside = fixture.temp.path().join("outside.txt");
    fs::write(&outside, "Private outside content").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, fixture.root.join("alias.txt")).unwrap();
    for client in fixture.clients() {
        #[cfg(unix)]
        assert_eq!(
            client
                .execute(client.prepare(Command::CreateKnowledgeCollection {
                    definition: definition(Source::Worktree {
                        worktree: fixture.worktree,
                        path: "alias.txt".into()
                    }),
                    plugin: None,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        for source in [
            Source::File {
                path: fixture.root.to_str().unwrap().into(),
            },
            Source::Directory {
                path: outside.to_str().unwrap().into(),
            },
            Source::File {
                path: "relative.txt".into(),
            },
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::CreateKnowledgeCollection {
                        definition: definition(source),
                        plugin: None
                    }))
                    .await
                    .is_err()
            );
        }
        assert!(list(&client).await.is_empty());
    }
    fixture.close().await;
}

#[tokio::test]
async fn pauses_indexing_and_rechecks_citation_originals() {
    let fixture = Fixture::start().await;
    let path = fixture.root.join("paused.md");
    fs::write(&path, "# Original\n原始内容仍可检索\n").unwrap();
    for client in fixture.clients() {
        fs::write(&path, "# Original\n原始内容仍可检索\n").unwrap();
        let first = super::saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Worktree {
                        worktree: fixture.worktree,
                        path: "paused.md".into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        let page = found(&client, None, "检索", first.id).await;
        let citation = page.hits[0].citation.clone();
        let request = CitationRequest {
            context: false,
            collection: citation.collection,
            chunk: citation.chunk.clone(),
            revision: citation.revision.clone(),
            offset: 0,
            limit: 4096,
        };
        let admission = client
            .dispatch(client.prepare(Command::ReadKnowledgeCitation(request.clone())))
            .await
            .unwrap();
        assert!(!admission.receipt.durable);
        let Output::KnowledgeCitation(opened) = admission.completion.await.unwrap().unwrap() else {
            panic!("citation expected")
        };
        assert!(opened.text.contains("原始内容仍可检索"));
        assert_eq!(opened.worktree, Some(fixture.worktree));
        let ready = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == first.id)
            .unwrap();
        let paused = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: first.id,
                    expected_revision: first.revision,
                    paused: true,
                },
            )
            .await,
        );
        assert!(paused.paused);
        assert_eq!(paused.progress.status, Status::Paused);
        assert_eq!(paused.progress.generation, first.progress.generation + 1);
        assert_eq!(paused.progress.chunks, ready.progress.chunks);
        assert_eq!(paused.progress.completed, ready.progress.completed);
        let retained = search(&client, None, query("检索", first.id)).await;
        assert_eq!(retained.mode, Mode::Keyword);
        assert!(!retained.indexing);
        assert!(!retained.hits.is_empty());
        assert!(retained.hits[0].text.contains("原始内容仍可检索"));
        let Output::KnowledgeCitation(reopened) =
            execute(&client, Command::ReadKnowledgeCitation(request.clone())).await
        else {
            panic!("citation expected")
        };
        assert_eq!(reopened.citation, citation);
        assert!(reopened.text.contains("原始内容仍可检索"));
        let unchanged = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: first.id,
                    expected_revision: paused.revision,
                    paused: true,
                },
            )
            .await,
        );
        assert_eq!(unchanged.revision, paused.revision);
        assert_eq!(unchanged.progress.generation, paused.progress.generation);
        fs::write(&path, "# Updated\n新的知识记录\n").unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadKnowledgeCitation(request)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::PauseKnowledgeCollection {
                    id: first.id,
                    expected_revision: first.revision,
                    paused: false,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let resumed = super::saved(
            execute(
                &client,
                Command::PauseKnowledgeCollection {
                    id: first.id,
                    expected_revision: paused.revision,
                    paused: false,
                },
            )
            .await,
        );
        assert!(!resumed.paused);
        assert_eq!(resumed.progress.status, Status::Pending);
        let updated = found(&client, None, "知识", first.id).await;
        assert!(updated.hits[0].text.contains("新的知识记录"));
        execute(
            &client,
            Command::DeleteKnowledgeCollection {
                id: first.id,
                expected_revision: resumed.revision,
            },
        )
        .await;
    }
    fixture.close().await;
}
