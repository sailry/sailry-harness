use super::*;
use crate::file_fixture::Fixture;

async fn query(fixture: &Fixture, client: &Client, options: FileSearch) -> SearchResults {
    let request = client.prepare(Command::SearchFiles {
        worktree: fixture.worktree,
        options,
    });
    let admission = client.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    let Output::SearchResults(result) = admission.completion.await.unwrap().unwrap() else {
        panic!("search results expected")
    };
    result
}

#[tokio::test]
async fn shares_modes_context_types_and_pagination() {
    let fixture = Fixture::start().await;
    std::fs::create_dir_all(fixture.root.join("src/nested")).unwrap();
    std::fs::write(fixture.root.join(".gitignore"), "*.ignored\n/private/\n").unwrap();
    std::fs::write(
        fixture.root.join("src/.gitignore"),
        "skip.rs\n!keep.ignored\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/main.rs"),
        "before\r\nneedle\r\nbetween\r\nneedle\r\nafter\r\n",
    )
    .unwrap();
    std::fs::write(fixture.root.join("src/extra.js"), "needle\nneedle\n").unwrap();
    std::fs::write(fixture.root.join("src/nested/script.py"), "first\nsecond\n").unwrap();
    std::fs::write(fixture.root.join("src/nested/skip.rs"), "needle hidden").unwrap();
    std::fs::write(fixture.root.join("src/keep.ignored"), "needle preserved").unwrap();
    std::fs::write(fixture.root.join("src/drop.ignored"), "needle hidden").unwrap();
    for client in fixture.clients() {
        let options = FileSearch {
            query: "needle".into(),
            path: "src".into(),
            context: 1,
            file_type: Some("rust".into()),
            limit: 2,
            ..Default::default()
        };
        let first = query(&fixture, &client, options.clone()).await;
        assert_eq!(
            first
                .matches
                .iter()
                .map(|row| (&*row.line, row.matched))
                .collect::<Vec<_>>(),
            [("before", false), ("needle", true)]
        );
        assert_eq!(first.next_offset, Some(2));
        let second = query(
            &fixture,
            &client,
            FileSearch {
                offset: 2,
                limit: 3,
                ..options
            },
        )
        .await;
        assert_eq!(
            second
                .matches
                .iter()
                .map(|row| row.line_number)
                .collect::<Vec<_>>(),
            [3, 4, 5]
        );
        assert!(second.next_offset.is_none());
        let files = query(
            &fixture,
            &client,
            FileSearch {
                query: "needle".into(),
                path: "src".into(),
                mode: SearchMode::Files,
                limit: 2,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(files.paths, ["src/extra.js", "src/keep.ignored"]);
        assert_eq!(files.next_offset, Some(2));
        let tail = query(
            &fixture,
            &client,
            FileSearch {
                query: "needle".into(),
                path: "src".into(),
                mode: SearchMode::Files,
                offset: 2,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(tail.paths, ["src/main.rs"]);
        let counts = query(
            &fixture,
            &client,
            FileSearch {
                query: "needle".into(),
                path: "src".into(),
                mode: SearchMode::Count,
                file_type: Some("js".into()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            counts.counts,
            [SearchCount {
                path: "src/extra.js".into(),
                count: 2
            }]
        );
        for (result, group) in [(files, "paths"), (counts, "counts"), (second, "matches")] {
            let json = serde_json::to_value(result).unwrap();
            assert!(json.get(group).is_some());
            for other in ["matches", "paths", "counts"]
                .into_iter()
                .filter(|other| *other != group)
            {
                assert!(json.get(other).is_none());
            }
        }
        let multiline = query(
            &fixture,
            &client,
            FileSearch {
                query: "first.*second".into(),
                path: "src/nested".into(),
                regex: true,
                multiline: true,
                file_type: Some("python".into()),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(
            multiline
                .matches
                .iter()
                .map(|row| row.line.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        for path in ["src/drop.ignored", "src/nested/skip.rs"] {
            assert!(
                query(
                    &fixture,
                    &client,
                    FileSearch {
                        query: "needle".into(),
                        path: path.into(),
                        ..Default::default()
                    }
                )
                .await
                .matches
                .is_empty()
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn shares_newest_first_discovery_pages() {
    let fixture = Fixture::start().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    std::fs::write(fixture.root.join(".gitignore"), "src/ignored.rs\n").unwrap();
    for (name, modified) in [
        ("a.rs", 2000),
        ("b.rs", 2000),
        ("old.rs", 1000),
        ("ignored.rs", 3000),
    ] {
        let path = fixture.root.join("src").join(name);
        std::fs::write(&path, "").unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified)),
            )
            .unwrap();
    }
    for client in fixture.clients() {
        for (offset, limit, expected, next) in [
            (0, 1, vec!["src/a.rs"], Some(1)),
            (1, 2, vec!["src/b.rs", "src/old.rs"], None),
        ] {
            let Output::FileNames(page) = client
                .execute(client.prepare(Command::FindFiles {
                    worktree: fixture.worktree,
                    globs: vec!["**/*.rs".into()],
                    path: "src".into(),
                    limit,
                    offset,
                }))
                .await
                .unwrap()
            else {
                panic!("file names expected")
            };
            assert_eq!(page.paths, expected);
            assert_eq!(page.next_offset, next);
            assert_eq!(page.offset, offset);
            assert!(!page.partial);
        }
    }
    fixture.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn confines_symlinked_rules_and_large_files() {
    let fixture = Fixture::start().await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("rules"), "*.rs\nprivate credential\n").unwrap();
    std::fs::write(outside.path().join("secret.rs"), "needle outside").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("rules"),
        fixture.root.join(".gitignore"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret.rs"),
        fixture.root.join("secret.rs"),
    )
    .unwrap();
    std::fs::write(fixture.root.join("visible.rs"), "needle inside").unwrap();
    std::fs::File::create(fixture.root.join("huge.rs"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    for client in fixture.clients() {
        let result = query(
            &fixture,
            &client,
            FileSearch {
                query: "needle".into(),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line, "needle inside");
        assert!(result.partial && result.next_offset.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.path == "huge.rs" && item.message.contains("64 MiB"))
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.message.contains("not a regular file"))
        );
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private credential")
        );
    }
    fixture.close().await;
}
