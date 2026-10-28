use crate::file_fixture::Fixture;
use sailry_client::Client;
use sailry_protocol::*;

async fn page(client: &Client, worktree: WorktreeId, path: &str, offset: usize) -> FileLines {
    let Output::FileLines(lines) = client
        .execute(client.prepare(Command::ReadFileLines {
            worktree,
            path: path.into(),
            offset,
            limit: 2,
            expected_revision: None,
        }))
        .await
        .unwrap()
    else {
        panic!("file lines expected")
    };
    lines
}

#[tokio::test]
async fn replays_mixed_newline_edits_with_raw_revisions() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let path = format!("mixed-{index}.txt");
        let original = "keep\r\none\r\ntwo\r\nmiddle\none\ntwo\nlast\r\n";
        std::fs::write(fixture.root.join(&path), original).unwrap();
        let request = client.prepare(Command::EditFile {
            worktree: fixture.worktree,
            path: path.clone(),
            old_text: None,
            new_text: None,
            replace_all: false,
            edits: Some(vec![
                FileEdit {
                    old_text: "one\ntwo\n".into(),
                    new_text: "A\nB\nC\n".into(),
                    replace_all: true,
                },
                FileEdit {
                    old_text: "last".into(),
                    new_text: "final\nextra".into(),
                    replace_all: false,
                },
            ]),
            expected_revision: blake3::hash(original.as_bytes()).to_hex().to_string(),
        });
        let expected = "keep\r\nA\r\nB\r\nC\r\nmiddle\nA\nB\nC\nfinal\r\nextra\r\n";
        let output = client.execute(request.clone()).await.unwrap();
        let Output::FileWritten(written) = &output else {
            panic!("file written expected")
        };
        assert_eq!(
            written.revision,
            blake3::hash(expected.as_bytes()).to_hex().to_string()
        );
        assert_eq!(
            std::fs::read(fixture.root.join(&path)).unwrap(),
            expected.as_bytes()
        );
        assert_eq!(client.execute(request).await.unwrap(), output);
        let stale = client.prepare(Command::EditFile {
            worktree: fixture.worktree,
            path: path.clone(),
            old_text: Some("final".into()),
            new_text: Some("stale".into()),
            replace_all: false,
            edits: None,
            expected_revision: blake3::hash(expected.replace("\r\n", "\n").as_bytes())
                .to_hex()
                .to_string(),
        });
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read(fixture.root.join(&path)).unwrap(),
            expected.as_bytes()
        );
    }
    fixture.close().await;
}

fn changes(worktree: WorktreeId, path: &str, text: &str) -> [Command; 2] {
    let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
    [
        Command::WriteFile {
            worktree,
            path: path.into(),
            text: "Replacement".into(),
            expected_revision: Some(revision.clone()),
        },
        Command::EditFile {
            worktree,
            path: path.into(),
            old_text: Some(text.into()),
            new_text: Some("Replacement".into()),
            replace_all: false,
            edits: None,
            expected_revision: revision,
        },
    ]
}

#[tokio::test]
async fn pages_and_edits_large_files() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let path = format!("large-{index}.txt");
        let original = (0..5000)
            .map(|line| format!("Line {line}: unchanged Unicode content 中文\r\n"))
            .collect::<String>();
        assert!(original.len() > MAX_FILE_BYTES);
        std::fs::write(fixture.root.join(&path), &original).unwrap();
        let lines = page(client, fixture.worktree, &path, 3000).await;
        assert_eq!(lines.lines[0].number, 3001);
        assert_eq!(
            lines.lines[0].text,
            "Line 3000: unchanged Unicode content 中文\r\n"
        );
        assert_eq!(lines.next_offset, Some(3002));
        assert_eq!(
            lines.revision,
            blake3::hash(original.as_bytes()).to_hex().to_string()
        );
        assert_eq!(
            page(client, fixture.worktree, &path, 4999)
                .await
                .next_offset,
            None
        );
        let request = client.prepare(Command::EditFile {
            worktree: fixture.worktree,
            path: path.clone(),
            old_text: Some(lines.lines[0].text.clone()),
            new_text: Some("Edited line\r\n".into()),
            replace_all: false,
            edits: None,
            expected_revision: lines.revision.clone(),
        });
        let output = client.execute(request.clone()).await.unwrap();
        let updated = original.replacen(&lines.lines[0].text, "Edited line\r\n", 1);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
            updated
        );
        assert!(matches!(output, Output::FileWritten(_)));
        assert_eq!(client.execute(request).await.unwrap(), output);
        let mut stale = client.prepare(Command::EditFile {
            worktree: fixture.worktree,
            path: path.clone(),
            old_text: Some("Edited line\r\n".into()),
            new_text: Some("Stale change\r\n".into()),
            replace_all: false,
            edits: None,
            expected_revision: lines.revision,
        });
        assert_eq!(
            client.execute(stale.clone()).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        stale.id = RequestId::new();
        if let Command::EditFile {
            old_text,
            expected_revision,
            ..
        } = &mut stale.command
        {
            *old_text = Some("unchanged Unicode".into());
            *expected_revision = blake3::hash(updated.as_bytes()).to_hex().to_string();
        }
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
            updated
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn pages_files_beyond_the_edit_budget() {
    use std::io::Write;

    let fixture = Fixture::empty().await;
    let header = (1..=40)
        .map(|number| format!("Header {number:02}\r\n"))
        .collect::<String>();
    let mut file = std::fs::File::create(fixture.root.join("generated.txt")).unwrap();
    let mut hash = blake3::Hasher::new();
    file.write_all(header.as_bytes()).unwrap();
    hash.update(header.as_bytes());
    let body = [b'x'; 8192];
    for _ in 0..=MAX_EDIT_BYTES / body.len() {
        file.write_all(&body).unwrap();
        hash.update(&body);
    }
    file.write_all(b"\nlast\n").unwrap();
    hash.update(b"\nlast\n");
    let size = file.metadata().unwrap().len();
    assert!(size > MAX_EDIT_BYTES as u64);
    drop(file);
    let revision = hash.finalize().to_hex().to_string();
    for client in fixture.clients() {
        for offset in [0, 20] {
            let Output::FileLines(page) = client
                .execute(client.prepare(Command::ReadFileLines {
                    worktree: fixture.worktree,
                    path: "generated.txt".into(),
                    offset,
                    limit: 20,
                    expected_revision: Some(revision.clone()),
                }))
                .await
                .unwrap()
            else {
                panic!("file lines expected")
            };
            assert_eq!(page.size, size);
            assert_eq!(page.revision, revision);
            assert_eq!(page.lines.len(), 20);
            assert_eq!(page.next_offset, Some(offset + 20));
            for (index, line) in page.lines.iter().enumerate() {
                let number = offset + index + 1;
                assert_eq!(line.number, number);
                assert_eq!(line.text, format!("Header {number:02}\r\n"));
            }
        }
        let page = page(&client, fixture.worktree, "generated.txt", 41).await;
        assert_eq!(page.revision, revision);
        assert_eq!(
            page.lines,
            [FileLine {
                number: 42,
                text: "last\n".into()
            }]
        );
        assert_eq!(page.next_offset, None);
    }
    fixture.close().await;
}

#[tokio::test]
async fn batches_edits_with_durable_replay() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let path = format!("batch-{index}.txt");
        let original = "First Repeat Repeat Last\r\n";
        let expected = blake3::hash(original.as_bytes()).to_hex().to_string();
        std::fs::write(fixture.root.join(&path), original).unwrap();
        let request = client.prepare(Command::EditFile {
            worktree: fixture.worktree,
            path: path.clone(),
            old_text: None,
            new_text: None,
            replace_all: false,
            edits: Some(vec![
                FileEdit {
                    old_text: "Last".into(),
                    new_text: "Repeat".into(),
                    replace_all: false,
                },
                FileEdit {
                    old_text: "Repeat".into(),
                    new_text: "中文".into(),
                    replace_all: true,
                },
            ]),
            expected_revision: expected.clone(),
        });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
            "First 中文 中文 Repeat\r\n"
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        let mut stale = request;
        stale.id = RequestId::new();
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn shares_revision_checked_move_and_trash() {
    let fixture = Fixture::empty().await;
    for (index, client) in fixture.clients().iter().enumerate() {
        let from = format!("source-{index}.txt");
        let to = format!("moved-{index}.txt");
        let expected = blake3::hash(b"Original").to_hex().to_string();
        std::fs::write(fixture.root.join(&from), "Original").unwrap();
        let moved = client.prepare(Command::MoveFile {
            worktree: fixture.worktree,
            from: from.clone(),
            to: to.clone(),
            expected_revision: expected.clone(),
        });
        let admission = client.dispatch(moved.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        assert_eq!(
            result,
            Output::EntryRenamed {
                from: from.clone(),
                to: to.clone()
            }
        );
        assert_eq!(client.execute(moved).await.unwrap(), result);
        assert!(!fixture.root.join(&from).exists());
        let trashed = client.prepare(Command::DeleteFile {
            worktree: fixture.worktree,
            path: to.clone(),
            expected_revision: expected,
        });
        let admission = client.dispatch(trashed.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let result = admission.completion.await.unwrap().unwrap();
        assert_eq!(result, Output::EntryTrashed { path: to.clone() });
        assert!(!fixture.root.join(&to).exists());
        std::fs::write(fixture.root.join(&to), "Later manual content").unwrap();
        assert_eq!(client.execute(trashed).await.unwrap(), result);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&to)).unwrap(),
            "Later manual content"
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn finds_names_without_reading_contents() {
    let fixture = Fixture::empty().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    std::fs::create_dir(fixture.root.join(".git")).unwrap();
    std::fs::write(fixture.root.join("src/binary.rs"), b"\xff\0").unwrap();
    std::fs::write(
        fixture.root.join("src/text.rs"),
        "No filename match in contents",
    )
    .unwrap();
    std::fs::write(fixture.root.join(".git/private.rs"), "private").unwrap();
    for (path, seconds) in [("src/text.rs", 2000), ("src/binary.rs", 1000)] {
        std::fs::File::options()
            .write(true)
            .open(fixture.root.join(path))
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
            )
            .unwrap();
    }
    for client in fixture.clients() {
        let Output::FileNames(found) = client
            .execute(client.prepare(Command::FindFiles {
                worktree: fixture.worktree,
                globs: vec!["**/*.rs".into()],
                path: String::new(),
                limit: MAX_FIND_MATCHES,
                offset: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("file names expected")
        };
        assert_eq!(found.paths, ["src/text.rs", "src/binary.rs"]);
        assert!(!found.truncated);
        assert_eq!(
            std::fs::read(fixture.root.join("src/binary.rs")).unwrap(),
            b"\xff\0"
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn rejects_paths_outside_worktree() {
    let fixture = Fixture::empty().await;
    for client in fixture.clients() {
        for path in ["../outside.txt", "/outside.txt"] {
            for command in [
                Command::ReadFileLines {
                    worktree: fixture.worktree,
                    path: path.into(),
                    offset: 0,
                    limit: 2,
                    expected_revision: None,
                },
                Command::EditFile {
                    worktree: fixture.worktree,
                    path: path.into(),
                    old_text: Some("old".into()),
                    new_text: Some("new".into()),
                    replace_all: false,
                    edits: None,
                    expected_revision: "a".repeat(64),
                },
            ] {
                assert!(client.execute(client.prepare(command)).await.is_err());
            }
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_git_metadata() {
    let fixture = Fixture::empty().await;
    let path = ".git/config";
    let original = "[core]\n\tbare = false\n";
    std::fs::create_dir(fixture.root.join(".git")).unwrap();
    std::fs::write(fixture.root.join(path), original).unwrap();
    for client in fixture.clients() {
        let lines = page(&client, fixture.worktree, path, 0).await;
        assert_eq!(
            lines
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            original
        );
        assert_eq!(
            lines.revision,
            blake3::hash(original.as_bytes()).to_hex().to_string()
        );
        for command in changes(fixture.worktree, path, original) {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(path)).unwrap(),
                original
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn protects_node_profile() {
    let fixture = Fixture::empty().await;
    let profile = fixture.node.profile();
    let protected = profile.join("mutation-protection.txt");
    let original = "Protected test content";
    std::fs::write(&protected, original).unwrap();
    let root = profile.parent().unwrap();
    let clients = fixture.clients();
    let worktree = crate::file_fixture::register(&clients[0], root).await;
    let path = format!(
        "{}/mutation-protection.txt",
        profile.file_name().unwrap().to_str().unwrap()
    );
    for client in clients {
        for command in changes(worktree, &path, original) {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert_eq!(std::fs::read_to_string(&protected).unwrap(), original);
        }
    }
    fixture.close().await;
}
