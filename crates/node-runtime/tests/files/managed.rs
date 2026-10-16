use super::*;
use crate::file_fixture::Fixture;
use sailry_link::CancellationToken;
use std::path::Path;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

fn initialize(root: &Path) {
    let repository = git2::Repository::init(root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file.txt"), "Initial content").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
}

async fn checkout(fixture: &Fixture, client: &Client) -> Worktree {
    initialize(&fixture.root);
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let project = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.id == fixture.worktree)
        .unwrap()
        .project
        .unwrap();
    let Output::GitStatus(status) = execute(
        client,
        Command::InspectGit {
            worktree: fixture.worktree,
        },
    )
    .await
    else {
        panic!("Git status expected")
    };
    let Output::Worktree(tree) = execute(
        client,
        Command::CreateManagedWorktree {
            project,
            source: fixture.worktree,
            branch: "manual-files".into(),
            expected_head: status.head.unwrap(),
            expected_index: status.index_revision.unwrap(),
            include_changes: false,
        },
    )
    .await
    else {
        panic!("worktree expected")
    };
    tree
}

#[tokio::test]
async fn shares_manual_operations_without_root_grants() {
    let fixture = Fixture::empty().await;
    let clients = fixture.clients();
    let tree = checkout(&fixture, &clients[0]).await;
    let base = format!(".sailry/worktrees/{}", tree.id);
    let metadata = std::fs::read(Path::new(&tree.path).join(".git")).unwrap();
    for (index, client) in clients.iter().enumerate() {
        for path in [".sailry/worktrees", base.as_str()] {
            let Output::Directory(page) = execute(
                client,
                Command::ListDirectory {
                    worktree: fixture.worktree,
                    path: path.into(),
                    after: None,
                },
            )
            .await
            else {
                panic!("directory expected")
            };
            assert!(!page.entries.is_empty());
        }
        let directory = format!("{base}/manual-{index}");
        execute(
            client,
            Command::CreateDirectory {
                worktree: fixture.worktree,
                path: directory.clone(),
            },
        )
        .await;
        let path = format!("{directory}/note.txt");
        let created = client.prepare(Command::WriteFile {
            worktree: fixture.worktree,
            path: path.clone(),
            text: "Original note".into(),
            expected_revision: None,
        });
        let Output::FileWritten(written) = client.execute(created.clone()).await.unwrap() else {
            panic!("written file expected")
        };
        assert_eq!(
            client.execute(created).await.unwrap(),
            Output::FileWritten(written.clone())
        );
        let Output::FileLines(lines) = execute(
            client,
            Command::ReadFileLines {
                worktree: fixture.worktree,
                path: path.clone(),
                offset: 0,
                limit: 10,
            },
        )
        .await
        else {
            panic!("file lines expected")
        };
        assert_eq!(lines.lines[0].text, "Original note");
        let Output::FileWritten(edited) = execute(
            client,
            Command::EditFile {
                worktree: fixture.worktree,
                path: path.clone(),
                old_text: Some("Original".into()),
                new_text: Some("Edited".into()),
                replace_all: false,
                edits: None,
                expected_revision: written.revision.clone(),
            },
        )
        .await
        else {
            panic!("edited file expected")
        };
        let replacement = "Saved document\n".repeat(MAX_FILE_BYTES / 8);
        let staged = client
            .stage_document(
                fixture.worktree,
                &path,
                &replacement,
                &edited.revision,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
            "Edited note"
        );
        execute_upload(client, staged.request).await;
        let content = client
            .read_document(fixture.worktree, &path, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(content.text, replacement);
        assert!(!content.truncated);
        let revision = content.revision.unwrap();
        let notebook_path = format!("{directory}/note.ipynb");
        let notebook_text =
            serde_json::json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[
                {"id":"fixture","cell_type":"markdown","source":"Before","metadata":{}}
            ]})
            .to_string();
        let Output::FileWritten(notebook_file) = execute(
            client,
            Command::WriteFile {
                worktree: fixture.worktree,
                path: notebook_path.clone(),
                text: notebook_text,
                expected_revision: None,
            },
        )
        .await
        else {
            panic!("notebook file expected")
        };
        execute(
            client,
            Command::EditNotebook {
                worktree: fixture.worktree,
                edit: notebook::Edit {
                    path: notebook_path.clone(),
                    expected_revision: notebook_file.revision,
                    edits: vec![notebook::Change::Replace {
                        index: 0,
                        source: "After".into(),
                    }],
                },
            },
        )
        .await;
        let Output::NotebookPage(page) = execute(
            client,
            Command::ReadNotebook {
                worktree: fixture.worktree,
                options: notebook::Read {
                    path: notebook_path,
                    offset: 0,
                    limit: 10,
                },
            },
        )
        .await
        else {
            panic!("notebook page expected")
        };
        assert_eq!(page.cells[0].value["source"], "After");
        let Output::FileDownload(download) = execute(
            client,
            Command::DownloadFile {
                worktree: fixture.worktree,
                path: path.clone(),
            },
        )
        .await
        else {
            panic!("download expected")
        };
        let mut bytes = Vec::new();
        client
            .download(&download, &mut bytes, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(bytes, replacement.as_bytes());
        let Output::FileNames(names) = execute(
            client,
            Command::FindFiles {
                worktree: fixture.worktree,
                globs: vec!["**/note.txt".into()],
                path: base.clone(),
                limit: 500,
                offset: 0,
            },
        )
        .await
        else {
            panic!("file names expected")
        };
        assert!(names.paths.contains(&path));
        let Output::SearchResults(results) = execute(
            client,
            Command::SearchFiles {
                worktree: fixture.worktree,
                options: FileSearch {
                    query: "Saved document".into(),
                    path: directory.clone(),
                    mode: SearchMode::Files,
                    ..Default::default()
                },
            },
        )
        .await
        else {
            panic!("search results expected")
        };
        assert_eq!(results.paths.as_slice(), std::slice::from_ref(&path));
        let copied = format!("{directory}/copy.txt");
        execute(
            client,
            Command::CopyEntry {
                worktree: fixture.worktree,
                from: path.clone(),
                to: copied.clone(),
            },
        )
        .await;
        let renamed = format!("{directory}/renamed.txt");
        execute(
            client,
            Command::RenameEntry {
                worktree: fixture.worktree,
                from: copied.clone(),
                to: renamed.clone(),
            },
        )
        .await;
        let moved = format!("{directory}/moved.txt");
        execute(
            client,
            Command::MoveFile {
                worktree: fixture.worktree,
                from: renamed.clone(),
                to: moved.clone(),
                expected_revision: revision.clone(),
            },
        )
        .await;
        let exported = format!("exported-{index}.txt");
        execute(
            client,
            Command::CopyEntryTo {
                source: fixture.worktree,
                worktree: tree.id,
                from: moved.clone(),
                to: exported.clone(),
            },
        )
        .await;
        let imported = format!("{directory}/imported.txt");
        execute(
            client,
            Command::MoveEntryTo {
                source: tree.id,
                worktree: fixture.worktree,
                from: exported,
                to: imported.clone(),
            },
        )
        .await;
        assert_eq!(std::fs::read(fixture.root.join(&imported)).unwrap(), bytes);
        assert_eq!(
            client
                .execute(client.prepare(Command::WriteFile {
                    worktree: fixture.worktree,
                    path: path.clone(),
                    text: "Do not overwrite".into(),
                    expected_revision: Some(edited.revision),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        execute(
            client,
            Command::DeleteFile {
                worktree: fixture.worktree,
                path: imported.clone(),
                expected_revision: revision.clone(),
            },
        )
        .await;
        let Output::FileDownload(checked) = execute(
            client,
            Command::DownloadFile {
                worktree: fixture.worktree,
                path: moved.clone(),
            },
        )
        .await
        else {
            panic!("download expected")
        };
        execute(
            client,
            Command::CancelFileTransfer {
                stream: checked.stream,
            },
        )
        .await;
        execute(
            client,
            Command::TrashFile {
                worktree: fixture.worktree,
                path: moved,
                expected_revision: checked.revision,
                expected_stamp: checked.stamp,
            },
        )
        .await;
        execute(
            client,
            Command::TrashEntry {
                worktree: fixture.worktree,
                path: directory.clone(),
            },
        )
        .await;
        assert!(!fixture.root.join(directory).exists());
        for command in [
            Command::TrashEntry {
                worktree: fixture.worktree,
                path: base.clone(),
            },
            Command::RenameEntry {
                worktree: fixture.worktree,
                from: base.clone(),
                to: "relocated".into(),
            },
            Command::CopyEntry {
                worktree: fixture.worktree,
                from: base.clone(),
                to: "copied-tree".into(),
            },
            Command::CreateDirectory {
                worktree: fixture.worktree,
                path: base.clone(),
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(Command::WriteFile {
                    worktree: fixture.worktree,
                    path: format!("{base}/.git"),
                    text: "Forbidden".into(),
                    expected_revision: None,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            read(client, WorktreeId::new(), &format!("{base}/file.txt"))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            read(client, tree.id, "../file.txt").await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        let mut wrong_node = client.prepare(Command::ReadFile {
            worktree: fixture.worktree,
            path: format!("{base}/file.txt"),
        });
        wrong_node.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong_node).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            std::fs::read(Path::new(&tree.path).join(".git")).unwrap(),
            metadata
        );
        assert_eq!(
            std::fs::read(fixture.root.join("file.txt")).unwrap(),
            b"Initial content"
        );
    }
    fixture.close().await;
}

async fn execute_upload(client: &Client, request: Request) {
    let admitted = client.dispatch(request.clone()).await.unwrap();
    assert!(admitted.receipt.durable);
    let output = admitted.completion.await.unwrap().unwrap();
    assert!(matches!(&output, Output::FileWritten(_)));
    assert_eq!(client.execute(request).await.unwrap(), output);
}
