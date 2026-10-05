use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::office;

async fn other_worktree(client: &Client, root: &Path) -> WorktreeId {
    fs::create_dir(root).unwrap();
    let Output::Project(project) = execute(
        client,
        Command::RegisterProject {
            name: "Other files".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(project.id))
        .unwrap()
        .id
}

async fn denied(client: &Client, context: &Context, command: Command) {
    let request = client.prepare(command).with_plugin(context.clone());
    let error = client.execute(request.clone()).await.unwrap_err();
    assert_eq!(
        error.code,
        ErrorCode::PermissionDenied,
        "{request:?}: {error:?}"
    );
    if request.command.durable() {
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(error)))
        );
    }
}

async fn completed(client: &Client, request: &Request) -> Output {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let RequestOutcome::Completed(result) = client.outcome(request).await.unwrap() {
                break result.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn scopes_document_streams_and_recovers_publication() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let other_root = directory.path().join("other");
        let other = other_worktree(&client, &other_root).await;
        let original = "Document 中文 🙂\n".repeat(MAX_FILE_BYTES / 10);
        assert!(original.len() > MAX_FILE_BYTES);
        fs::write(root.join("document.txt"), &original).unwrap();
        fs::write(other_root.join("document.txt"), "Other worktree").unwrap();
        let read_only = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadFiles],
        )
        .await;
        let content = client
            .read_document_scoped(&read_only, "document.txt", CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(content.text, original);
        assert!(!content.truncated);
        let revision = content.revision.unwrap();
        assert_eq!(
            revision,
            blake3::hash(original.as_bytes()).to_hex().to_string()
        );
        assert_eq!(
            client
                .stage_document_scoped(
                    &read_only,
                    "document.txt",
                    "Denied",
                    &revision,
                    CancellationToken::new()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for command in [
            Command::DownloadFile {
                worktree: other,
                path: "document.txt".into(),
            },
            Command::UploadFile(FileUploadSpec {
                worktree: other,
                path: "document.txt".into(),
                size: 6,
                revision: blake3::hash(b"Denied").to_hex().to_string(),
                expected_revision: None,
            }),
        ] {
            denied(&client, &read_only, command).await;
        }
        let read_request = client
            .prepare(Command::DownloadFile {
                worktree,
                path: "document.txt".into(),
            })
            .with_plugin(read_only.clone());
        let Output::FileDownload(download) = client.execute(read_request).await.unwrap() else {
            panic!("download expected")
        };
        // The native owner may release only its own opaque transfer, without
        // granting a plugin access to arbitrary caller-owned stream handles.
        denied(
            &client,
            &read_only,
            Command::CancelFileTransfer {
                stream: download.stream,
            },
        )
        .await;
        assert_eq!(
            execute(
                &client,
                Command::CancelFileTransfer {
                    stream: download.stream
                }
            )
            .await,
            Output::FileTransferCancelled {
                stream: download.stream
            }
        );

        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            1,
            &[Action::ReadFiles, Action::WriteFiles],
        )
        .await;
        let replacement = "Saved 中文 🙂\n".repeat(MAX_FILE_BYTES / 8);
        let staged = client
            .stage_document_scoped(
                &context,
                "document.txt",
                &replacement,
                &revision,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(staged.request.plugin.as_ref(), Some(&context));
        assert_eq!(
            fs::read_to_string(root.join("document.txt")).unwrap(),
            original
        );
        let Command::FinishFileUpload { stream, .. } = staged.request.command else {
            panic!("publication expected")
        };
        denied(
            &client,
            &context,
            Command::FinishFileUpload {
                worktree: other,
                path: "document.txt".into(),
                stream,
            },
        )
        .await;
        let wrong_path = client
            .prepare(Command::FinishFileUpload {
                worktree,
                path: "different.txt".into(),
                stream,
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(wrong_path).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        let request = staged.request;
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission.completion);
        let output = completed(&client, &request).await;
        assert!(
            matches!(&output,Output::FileWritten(written) if written.path == "document.txt" && written.revision == blake3::hash(replacement.as_bytes()).to_hex().as_str())
        );
        assert_eq!(
            fs::read_to_string(root.join("document.txt")).unwrap(),
            replacement
        );
        let restored = client
            .read_document_scoped(&context, "document.txt", CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(restored.text, replacement);
        assert!(!restored.truncated);
        fs::write(root.join("document.txt"), "External edit").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), output);
        assert_eq!(
            fs::read_to_string(root.join("document.txt")).unwrap(),
            "External edit"
        );
        assert_eq!(
            fs::read_to_string(other_root.join("document.txt")).unwrap(),
            "Other worktree"
        );
        assert!(!root.join("different.txt").exists());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn confines_directory_changes_to_both_captured_paths() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        let other_root = directory.path().join("other");
        let other = other_worktree(&client, &other_root).await;
        fs::create_dir(root.join("original")).unwrap();
        fs::write(root.join("original/note.txt"), "Source").unwrap();
        fs::write(other_root.join("note.txt"), "Other worktree").unwrap();
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadFiles],
        )
        .await;
        for command in [
            Command::CreateDirectory {
                worktree,
                path: "created".into(),
            },
            Command::RenameEntry {
                worktree,
                from: "original".into(),
                to: "renamed".into(),
            },
            Command::CopyEntry {
                worktree,
                from: "original".into(),
                to: "copied".into(),
            },
            Command::CopyEntryTo {
                source: worktree,
                worktree,
                from: "original".into(),
                to: "copied-to".into(),
            },
            Command::MoveEntryTo {
                source: worktree,
                worktree,
                from: "original".into(),
                to: "moved-to".into(),
            },
            Command::TrashEntry {
                worktree,
                path: "original".into(),
            },
            Command::TrashFile {
                worktree,
                path: "original/note.txt".into(),
                expected_revision: blake3::hash(b"Source").to_hex().to_string(),
                expected_stamp: "unchanged".into(),
            },
        ] {
            denied(&client, &context, command).await;
        }
        assert_eq!(
            fs::read_to_string(root.join("original/note.txt")).unwrap(),
            "Source"
        );
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            1,
            &[Action::ReadFiles, Action::WriteFiles],
        )
        .await;
        for command in [
            Command::CreateDirectory {
                worktree: other,
                path: "created".into(),
            },
            Command::RenameEntry {
                worktree: other,
                from: "note.txt".into(),
                to: "renamed.txt".into(),
            },
            Command::CopyEntry {
                worktree: other,
                from: "note.txt".into(),
                to: "copied.txt".into(),
            },
            Command::TrashEntry {
                worktree: other,
                path: "note.txt".into(),
            },
            Command::TrashFile {
                worktree: other,
                path: "note.txt".into(),
                expected_revision: blake3::hash(b"Other worktree").to_hex().to_string(),
                expected_stamp: "unchanged".into(),
            },
        ] {
            denied(&client, &context, command).await;
        }
        for (source, target) in [(other, worktree), (worktree, other), (other, other)] {
            for command in [
                Command::CopyEntryTo {
                    source,
                    worktree: target,
                    from: "note.txt".into(),
                    to: "cross.txt".into(),
                },
                Command::MoveEntryTo {
                    source,
                    worktree: target,
                    from: "note.txt".into(),
                    to: "cross.txt".into(),
                },
            ] {
                denied(&client, &context, command).await;
            }
        }
        for command in [
            Command::CreateDirectory {
                worktree,
                path: "created".into(),
            },
            Command::CopyEntry {
                worktree,
                from: "original".into(),
                to: "created/copied".into(),
            },
            Command::RenameEntry {
                worktree,
                from: "created/copied".into(),
                to: "created/renamed".into(),
            },
            Command::CopyEntryTo {
                source: worktree,
                worktree,
                from: "created/renamed".into(),
                to: "created/copied-to".into(),
            },
        ] {
            let request = client.prepare(command).with_plugin(context.clone());
            let admission = client.dispatch(request).await.unwrap();
            assert!(admission.receipt.durable);
            admission.completion.await.unwrap().unwrap();
        }
        let request = client
            .prepare(Command::MoveEntryTo {
                source: worktree,
                worktree,
                from: "created/copied-to".into(),
                to: "created/moved".into(),
            })
            .with_plugin(context.clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission.completion);
        let output = completed(&client, &request).await;
        assert_eq!(
            output,
            Output::EntryMoved {
                from: "created/copied-to".into(),
                to: "created/moved".into()
            }
        );
        assert!(!root.join("created/copied-to").exists());
        fs::write(root.join("created/moved/note.txt"), "External edit").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        assert_eq!(
            fs::read_to_string(root.join("created/moved/note.txt")).unwrap(),
            "External edit"
        );
        assert_eq!(
            fs::read_to_string(root.join("original/note.txt")).unwrap(),
            "Source"
        );
        assert_eq!(
            fs::read_to_string(other_root.join("note.txt")).unwrap(),
            "Other worktree"
        );
        assert!(!other_root.join("created").exists());
        assert!(!root.join("cross.txt").exists());
        assert!(!other_root.join("cross.txt").exists());
        for command in [
            Command::RenameEntry {
                worktree,
                from: "../other/note.txt".into(),
                to: "escaped.txt".into(),
            },
            Command::CopyEntry {
                worktree,
                from: "original/note.txt".into(),
                to: "../other/escaped.txt".into(),
            },
            Command::TrashEntry {
                worktree,
                path: "../other/note.txt".into(),
            },
        ] {
            assert!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .is_err()
            );
        }
        assert!(!other_root.join("escaped.txt").exists());
        assert_eq!(
            fs::read_to_string(other_root.join("note.txt")).unwrap(),
            "Other worktree"
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn office_reads_and_exports_require_grants() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/office/fixtures/report.docx"),
            root.join("report.docx"),
        )
        .unwrap();
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            0,
            &[Action::ReadFiles],
        )
        .await;
        let read = Command::ReadOffice {
            worktree,
            options: office::Read {
                path: "report.docx".into(),
                offset: 0,
            },
        };
        let output = client
            .execute(client.prepare(read.clone()).with_plugin(context.clone()))
            .await
            .unwrap();
        assert_eq!(output, execute(&client, read).await);
        let Output::OfficeContent(content) = output else {
            panic!("Office content expected")
        };
        assert!(
            content
                .sections
                .iter()
                .any(|section| section.text.contains("English text"))
        );
        let Output::OfficePreview(preview) = client
            .execute(
                client
                    .prepare(Command::PreviewOffice {
                        worktree,
                        path: "report.docx".into(),
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("Office preview expected")
        };
        assert_eq!(preview.download.worktree, worktree);
        assert_eq!(preview.download.path, "report.docx");
        let mut preview_bytes = Vec::new();
        client
            .download(
                &preview.download,
                &mut preview_bytes,
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert!(preview_bytes.starts_with(b"%PDF-"));
        let runtime = Command::OfficeRuntime { worktree };
        assert_eq!(
            client
                .execute(client.prepare(runtime.clone()).with_plugin(context.clone()))
                .await,
            client.execute(client.prepare(runtime)).await
        );
        let export = Command::ExportPdf {
            worktree,
            options: office::Export {
                source: "report.docx".into(),
                path: "export.pdf".into(),
                expected_revision: None,
            },
        };
        assert_eq!(
            client
                .execute(client.prepare(export.clone()).with_plugin(context))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(!root.join("export.pdf").exists());
        let context = install_actions(
            &client,
            &root.join("package"),
            worktree,
            1,
            &[Action::ReadFiles, Action::WriteFiles],
        )
        .await;
        for command in [
            Command::PreviewOffice {
                worktree: WorktreeId::new(),
                path: "report.docx".into(),
            },
            Command::ReadOffice {
                worktree: WorktreeId::new(),
                options: office::Read {
                    path: "report.docx".into(),
                    offset: 0,
                },
            },
            Command::ExportPdf {
                worktree: WorktreeId::new(),
                options: office::Export {
                    source: "report.docx".into(),
                    path: "wrong.pdf".into(),
                    expected_revision: None,
                },
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let request = client.prepare(export).with_plugin(context);
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission.completion);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(result, Output::OfficeWritten(_)));
        let bytes = fs::read(root.join("export.pdf")).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        fs::write(root.join("export.pdf"), "External replacement").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            fs::read_to_string(root.join("export.pdf")).unwrap(),
            "External replacement"
        );
        assert!(!root.join("wrong.pdf").exists());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
