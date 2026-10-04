use super::*;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn checks_owners() {
    let fixture = Fixture::start().await;
    let clients = fixture.clients();
    for (index, client) in clients.iter().enumerate() {
        let other = &clients[1 - index];
        let upload = prepare(client, spec(fixture.worktree, "owned", b"bytes")).await;
        assert!(other.open(upload.stream).await.is_err());
        assert!(
            other
                .execute(other.prepare(Command::CancelFileTransfer {
                    stream: upload.stream
                }))
                .await
                .is_err()
        );
        stage(client, &upload, b"bytes").await;
        assert!(other.execute(finish(other, &upload)).await.is_err());
        assert!(
            client
                .execute(client.prepare(Command::FinishAttachmentUpload {
                    worktree: WorktreeId::new(),
                    stream: upload.stream
                }))
                .await
                .is_err()
        );
        assert!(
            client
                .execute(client.prepare(Command::FinishFileUpload {
                    worktree: fixture.worktree,
                    path: "not-a-project-file".into(),
                    stream: upload.stream
                }))
                .await
                .is_err()
        );
        let attachment = publish(client, &upload).await;
        assert!(
            other
                .execute(other.prepare(Command::DiscardAttachment {
                    worktree: fixture.worktree,
                    attachment: attachment.id
                }))
                .await
                .is_err()
        );
        for command in [
            Command::ReadAttachment {
                worktree: WorktreeId::new(),
                attachment: attachment.id,
            },
            Command::DownloadAttachment {
                worktree: WorktreeId::new(),
                attachment: attachment.id,
            },
            Command::DiscardAttachment {
                worktree: WorktreeId::new(),
                attachment: attachment.id,
            },
        ] {
            assert!(client.execute(client.prepare(command)).await.is_err());
        }
        assert_eq!(download(client, &attachment).await, b"bytes");
        let path = format!("project-file-{index}");
        let Output::FileUpload(file) = client
            .execute(client.prepare(Command::UploadFile(FileUploadSpec {
                worktree: fixture.worktree,
                path: path.clone(),
                size: 5,
                revision: blake3::hash(b"bytes").to_hex().to_string(),
                expected_revision: None,
            })))
            .await
            .unwrap()
        else {
            panic!("file upload expected")
        };
        client
            .upload(&file, &mut &b"bytes"[..], CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert!(
            client
                .execute(client.prepare(Command::FinishAttachmentUpload {
                    worktree: fixture.worktree,
                    stream: file.stream
                }))
                .await
                .is_err()
        );
        client
            .execute(client.prepare(Command::FinishFileUpload {
                worktree: fixture.worktree,
                path,
                stream: file.stream,
            }))
            .await
            .unwrap();
    }
    fixture.close().await;
}

#[tokio::test]
async fn rejects_invalid_specs() {
    let fixture = Fixture::start().await;
    for client in fixture.clients() {
        let valid = spec(fixture.worktree, "file", b"bytes");
        for invalid in [
            Spec {
                worktree: WorktreeId::new(),
                ..valid.clone()
            },
            Spec {
                name: "../outside".into(),
                ..valid.clone()
            },
            Spec {
                size: MAX_BYTES + 1,
                ..valid.clone()
            },
            Spec {
                media_type: "image\n/png".into(),
                ..valid.clone()
            },
            Spec {
                revision: "not-a-hash".into(),
                ..valid.clone()
            },
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::UploadAttachment(invalid)))
                    .await
                    .is_err()
            );
        }
        let upload = prepare(&client, valid).await;
        let mut wrong = client.prepare(Command::UploadAttachment(upload.spec.clone()));
        wrong.target = NodeId([42; 32]);
        assert!(client.execute(wrong).await.is_err());
        client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: upload.stream,
            }))
            .await
            .unwrap();
    }
    fixture.close().await;
}

#[tokio::test]
async fn cancels_unpublished_streams() {
    let fixture = Fixture::start().await;
    let database =
        rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3")).unwrap();
    for client in fixture.clients() {
        let upload = prepare(&client, spec(fixture.worktree, "unfinished", &[0; 1024])).await;
        let mut stream = client.open(upload.stream).await.unwrap();
        stream.write_all(b"part").await.unwrap();
        client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: upload.stream,
            }))
            .await
            .unwrap();
        drop(stream);
        assert!(client.execute(finish(&client, &upload)).await.is_err());
        let upload = prepare(&client, spec(fixture.worktree, "cancelled", b"bytes")).await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            client
                .upload_attachment(&upload, &mut &b"bytes"[..], cancel, |_| {})
                .await
                .is_err()
        );
        assert!(client.open(upload.stream).await.is_err());
    }
    assert_eq!(count(&database, "attachments"), 0);
    fixture.close().await;
}
