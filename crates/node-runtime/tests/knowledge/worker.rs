use super::*;

async fn events(node: &Node) -> u64 {
    let bodies = node
        .with_storage(|db| {
            Box::pin(async move {
                let mut rows = db
                    .query("SELECT body FROM events ORDER BY cursor", ())
                    .await
                    .map_err(crate::knowledge::database_fixture::fault)?;
                let mut bodies = Vec::new();
                while let Some(row) = rows
                    .next()
                    .await
                    .map_err(crate::knowledge::database_fixture::fault)?
                {
                    bodies.push(
                        row.get::<Vec<u8>>(0)
                            .map_err(crate::knowledge::database_fixture::fault)?,
                    );
                }
                Ok(bodies)
            })
        })
        .await
        .unwrap();
    bodies
        .into_iter()
        .map(|body| serde_json::from_slice::<Event>(&body).unwrap())
        .filter(|event| matches!(event, Event::KnowledgeCollectionsChanged))
        .count() as u64
}

async fn ready(client: &Client, id: KnowledgeId, sources: u64) -> Collection {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let current = list(client)
                .await
                .into_iter()
                .find(|collection| collection.id == id)
                .unwrap();
            if current.progress.status == Status::Ready && current.progress.sources == sources {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("knowledge source monitoring completed")
}

#[tokio::test]
async fn monitors_changes_without_idle_or_build_churn() {
    let fixture = Fixture::start().await;
    for (number, client) in fixture.clients().into_iter().enumerate() {
        let directory = fixture.root.join(format!("documents-{number}"));
        fs::create_dir(&directory).unwrap();
        for child in ["target", ".git", "nested"] {
            fs::create_dir(directory.join(child)).unwrap();
        }
        let ignores = "target/\n*.tmp\n";
        fs::write(directory.join(".gitignore"), ignores).unwrap();
        fs::write(
            directory.join("nested/source.md"),
            "Original database reference",
        )
        .unwrap();
        let mut definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: format!("documents-{number}"),
        });
        definition.globs = vec!["**/*.md".into()];
        let collection = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            )
            .await,
        );
        let first = found(&client, None, "Original", collection.id).await;
        let original = ready(&client, collection.id, 1).await;
        assert_eq!(original.progress.skipped, 0);

        let before = events(&fixture.node).await;
        // A relevant ignore-file event causes an unchanged pass, while build and Git
        // writes must not reset progress or replace any indexed source.
        fs::write(directory.join(".gitignore"), ignores).unwrap();
        for number in 0..20 {
            fs::write(
                directory.join(format!("target/object-{number}.o")),
                "generated build output",
            )
            .unwrap();
            fs::write(
                directory.join(".git/index"),
                format!("fixture metadata {number}"),
            )
            .unwrap();
            fs::write(directory.join("scratch.tmp"), format!("ignored {number}")).unwrap();
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let unchanged = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == collection.id)
            .unwrap();
        assert_eq!(unchanged.progress, original.progress);
        assert_eq!(events(&fixture.node).await, before);
        let cached = search(&client, None, query("Original", collection.id)).await;
        assert!(!cached.indexing);
        assert_eq!(cached.hits[0].citation, first.hits[0].citation);
        fs::write(
            directory.join("nested/source.md"),
            "Updated reference with new source content",
        )
        .unwrap();
        let updated = found(&client, None, "Updated", collection.id).await;
        assert_ne!(
            updated.hits[0].citation.revision,
            first.hits[0].citation.revision
        );
        assert!(updated.hits[0].text.contains("new source content"));
        assert!(
            search(&client, None, query("Original", collection.id))
                .await
                .hits
                .is_empty()
        );
        fs::write(
            directory.join("nested/added.md"),
            "A newly discovered reference",
        )
        .unwrap();
        found(&client, None, "discovered", collection.id).await;
        assert_eq!(ready(&client, collection.id, 2).await.progress.completed, 2);
        fs::remove_file(directory.join("nested/source.md")).unwrap();
        ready(&client, collection.id, 1).await;
        assert!(
            search(&client, None, query("Updated", collection.id))
                .await
                .hits
                .is_empty()
        );
        execute(
            &client,
            Command::DeleteKnowledgeCollection {
                id: collection.id,
                expected_revision: collection.revision,
            },
        )
        .await;
    }
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn retains_index_when_directory_scan_is_incomplete() {
    let fixture = Fixture::start().await;
    for (number, client) in fixture.clients().into_iter().enumerate() {
        let directory = fixture.root.join(format!("retained-{number}"));
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("child")).unwrap();
        fs::write(directory.join("child/source.md"), "Retained indexed source").unwrap();
        let collection = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Directory {
                        path: directory.to_str().unwrap().into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        found(&client, None, "Retained", collection.id).await;
        let original = ready(&client, collection.id, 1).await;
        let rules = fixture.temp.path().join(format!("external-rules-{number}"));
        fs::write(&rules, "untrusted external rules").unwrap();
        std::os::unix::fs::symlink(&rules, directory.join("child/.gitignore")).unwrap();
        let paused = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let current = list(&client)
                    .await
                    .into_iter()
                    .find(|entry| entry.id == collection.id)
                    .unwrap();
                if current.progress.status == Status::Paused {
                    return current;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!("incomplete scan paused without pruning for controller {number}")
        });
        assert_eq!(paused.progress.sources, original.progress.sources);
        assert_eq!(paused.progress.chunks, original.progress.chunks);
        let retained = search(&client, None, query("Retained", collection.id)).await;
        assert_eq!(retained.hits.len(), 1);
        assert_eq!(
            retained.hits[0].citation.revision,
            blake3::hash(b"Retained indexed source").to_hex().as_str()
        );
        assert_eq!(
            fs::read_to_string(&rules).unwrap(),
            "untrusted external rules"
        );
        fs::remove_file(directory.join("child/.gitignore")).unwrap();
        ready(&client, collection.id, 1).await;
        execute(
            &client,
            Command::DeleteKnowledgeCollection {
                id: collection.id,
                expected_revision: collection.revision,
            },
        )
        .await;
    }
    fixture.close().await;
}
