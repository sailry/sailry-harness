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
        }))
        .await
        .unwrap()
    else {
        panic!("file lines expected")
    };
    lines
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
            old_text: text.into(),
            new_text: "Replacement".into(),
            expected_revision: revision,
        },
    ]
}

#[tokio::test]
async fn pages_and_edits_large_files() {
    let fixture = Fixture::start().await;
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
            old_text: lines.lines[0].text.clone(),
            new_text: "Edited line\r\n".into(),
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
            old_text: "Edited line\r\n".into(),
            new_text: "Stale change\r\n".into(),
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
            *old_text = "unchanged Unicode".into();
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
async fn finds_names_without_reading_contents() {
    let fixture = Fixture::start().await;
    std::fs::create_dir(fixture.root.join("src")).unwrap();
    std::fs::create_dir(fixture.root.join(".git")).unwrap();
    std::fs::write(fixture.root.join("src/binary.rs"), b"\xff\0").unwrap();
    std::fs::write(
        fixture.root.join("src/text.rs"),
        "No filename match in contents",
    )
    .unwrap();
    std::fs::write(fixture.root.join(".git/private.rs"), "private").unwrap();
    for client in fixture.clients() {
        let Output::FileNames(found) = client
            .execute(client.prepare(Command::FindFiles {
                worktree: fixture.worktree,
                globs: vec!["**/*.rs".into()],
            }))
            .await
            .unwrap()
        else {
            panic!("file names expected")
        };
        assert_eq!(found.paths, ["src/binary.rs", "src/text.rs"]);
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
    let fixture = Fixture::start().await;
    for client in fixture.clients() {
        for path in ["../outside.txt", "/outside.txt"] {
            for command in [
                Command::ReadFileLines {
                    worktree: fixture.worktree,
                    path: path.into(),
                    offset: 0,
                    limit: 2,
                },
                Command::EditFile {
                    worktree: fixture.worktree,
                    path: path.into(),
                    old_text: "old".into(),
                    new_text: "new".into(),
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
    let fixture = Fixture::start().await;
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
    let fixture = Fixture::start().await;
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
