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
async fn excludes_managed_checkouts() {
    let fixture = Fixture::start().await;
    let child = format!(".sailry/worktrees/{}", WorktreeId::new());
    std::fs::create_dir_all(fixture.root.join(&child)).unwrap();
    std::fs::write(fixture.root.join(&child).join("child.txt"), "needle child").unwrap();
    std::fs::write(fixture.root.join(".sailry/notes.txt"), "needle notes").unwrap();
    std::fs::write(fixture.root.join("main.txt"), "needle main").unwrap();
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
        assert_eq!(
            result
                .matches
                .iter()
                .map(|row| row.path.as_str())
                .collect::<Vec<_>>(),
            [".sailry/notes.txt", "main.txt"]
        );
        assert!(!result.partial);
        let Output::FileNames(names) = client
            .execute(client.prepare(Command::FindFiles {
                worktree: fixture.worktree,
                globs: vec!["**/*.txt".into()],
                path: String::new(),
                limit: 500,
                offset: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("file names expected")
        };
        assert_eq!(names.paths.len(), 2);
        assert!(
            names
                .paths
                .iter()
                .all(|path| !path.starts_with(".sailry/worktrees/"))
        );
        let Output::Directory(directory) = client
            .execute(client.prepare(Command::ListDirectory {
                worktree: fixture.worktree,
                path: ".sailry".into(),
                after: None,
            }))
            .await
            .unwrap()
        else {
            panic!("directory expected")
        };
        assert_eq!(
            directory
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["notes.txt"]
        );
        assert_eq!(directory.unsupported_names, 0);
        client
            .execute(client.prepare(Command::ReadFile {
                worktree: fixture.worktree,
                path: ".sailry/notes.txt".into(),
            }))
            .await
            .unwrap();
        for command in [
            Command::ReadFile {
                worktree: fixture.worktree,
                path: format!("{child}/child.txt"),
            },
            Command::WriteFile {
                worktree: fixture.worktree,
                path: format!("{child}/crossed.txt"),
                text: "crossed".into(),
                expected_revision: None,
            },
            Command::ListDirectory {
                worktree: fixture.worktree,
                path: ".sailry/worktrees".into(),
                after: None,
            },
            Command::SearchFiles {
                worktree: fixture.worktree,
                options: FileSearch {
                    query: "needle".into(),
                    path: child.clone(),
                    ..Default::default()
                },
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert!(!fixture.root.join(&child).join("crossed.txt").exists());
    }
    fixture.close().await;
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
async fn pages_encoded_text_despite_skipped_files() {
    let fixture = Fixture::start().await;
    std::fs::create_dir(fixture.root.join("encoded")).unwrap();
    let directory = fixture.root.join("encoded");
    std::fs::File::create(directory.join("a-large.txt"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    std::fs::write(directory.join("b-binary.txt"), b"\xd6\xd0\xce\xc4\0binary").unwrap();
    std::fs::write(directory.join("c-invalid.txt"), b"\xd6\xd0\xce\xc4\xff").unwrap();
    let denied = directory.join("d-denied.txt");
    std::fs::write(&denied, "not a query match").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o0)).unwrap();
    }
    let inaccessible = std::fs::File::open(&denied).is_err();
    std::fs::write(
        directory.join("e-gbk.txt"),
        b"\xd6\xd0\xce\xc4 first\r\n\xd6\xd0\xce\xc4 second\r\n",
    )
    .unwrap();
    let (encoded, _, failed) = encoding_rs::GB18030.encode("中文𠀀🙂\n");
    assert!(!failed);
    std::fs::write(directory.join("f-gb18030.txt"), encoded).unwrap();
    std::fs::write(directory.join("g-bom.txt"), "\u{feff}中文 UTF-8\n").unwrap();
    for client in fixture.clients() {
        let options = FileSearch {
            query: "中文".into(),
            path: "encoded".into(),
            limit: 2,
            ..Default::default()
        };
        for (offset, expected, next) in [
            (
                0,
                [
                    ("encoded/e-gbk.txt", 1, "中文 first"),
                    ("encoded/e-gbk.txt", 2, "中文 second"),
                ],
                Some(2),
            ),
            (
                2,
                [
                    ("encoded/f-gb18030.txt", 1, "中文𠀀🙂"),
                    ("encoded/g-bom.txt", 1, "中文 UTF-8"),
                ],
                None,
            ),
        ] {
            let page = query(
                &fixture,
                &client,
                FileSearch {
                    offset,
                    ..options.clone()
                },
            )
            .await;
            assert!(!page.partial, "{page:?}");
            assert_eq!(page.offset, offset);
            assert_eq!(page.next_offset, next);
            assert_eq!(page.truncated, next.is_some());
            assert_eq!(page.scanned_files, 7);
            assert_eq!(page.skipped, 3 + usize::from(inaccessible));
            assert_eq!(page.diagnostics.len(), page.skipped - 1);
            assert!(
                page.diagnostics
                    .iter()
                    .all(|entry| entry.path != "encoded/b-binary.txt")
            );
            assert_eq!(
                page.matches
                    .iter()
                    .map(|row| (row.path.as_str(), row.line_number, row.line.as_str()))
                    .collect::<Vec<_>>(),
                expected
            );
            for (path, message) in [
                ("encoded/a-large.txt", "64 MiB"),
                ("encoded/c-invalid.txt", "UTF-8 nor GB18030"),
            ] {
                assert!(
                    page.diagnostics
                        .iter()
                        .any(|entry| { entry.path == path && entry.message.contains(message) })
                );
            }
            if inaccessible {
                assert!(page.diagnostics.iter().any(|entry| {
                    entry.path == "encoded/d-denied.txt"
                        && entry.message.contains("filesystem operation failed")
                }));
            }
        }
        let anchored = query(
            &fixture,
            &client,
            FileSearch {
                query: "^中文.*second$".into(),
                regex: true,
                ..options
            },
        )
        .await;
        assert!(!anchored.partial);
        assert_eq!(anchored.matches.len(), 1);
        assert_eq!(anchored.matches[0].path, "encoded/e-gbk.txt");
        assert_eq!(anchored.matches[0].line_number, 2);
        assert_eq!(anchored.matches[0].line, "中文 second");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o600)).unwrap();
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
