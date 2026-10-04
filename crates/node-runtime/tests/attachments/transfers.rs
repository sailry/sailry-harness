use super::*;

#[tokio::test]
async fn preserves_bytes() {
    let fixture = Fixture::start().await;
    let database =
        rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3")).unwrap();
    let clients = fixture.clients();
    let large: Vec<_> = (0..10 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    for client in &clients {
        for (name, bytes) in [("资料 🙂.bin", large.as_slice()), ("empty", &[])] {
            let before = count(&database, "requests");
            let records = count(&database, "attachments");
            let upload = prepare(client, spec(fixture.worktree, name, bytes)).await;
            stage(client, &upload, bytes).await;
            assert_eq!(count(&database, "requests"), before);
            assert_eq!(count(&database, "attachments"), records);
            assert!(!fixture.root.join(name).exists());
            let attachment = publish(client, &upload).await;
            assert_eq!(count(&database, "requests"), before + 1);
            assert_eq!(count(&database, "attachments"), records + 1);
            for reader in &clients {
                assert_eq!(
                    reader
                        .execute(reader.prepare(read(&attachment)))
                        .await
                        .unwrap(),
                    Output::Attachment(attachment.clone())
                );
                assert_eq!(download(reader, &attachment).await, bytes);
            }
            assert_eq!(count(&database, "requests"), before + 1);
            assert_eq!(
                std::fs::read(
                    fixture
                        .node
                        .profile()
                        .join("attachments")
                        .join(attachment.id.to_string())
                )
                .unwrap(),
                bytes
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn checks_integrity() {
    let fixture = Fixture::start().await;
    for client in fixture.clients() {
        for bytes in [b"bad".as_slice(), b"same!", b"too many"] {
            let upload = prepare(&client, spec(fixture.worktree, "image", b"valid")).await;
            assert!(
                client
                    .upload_attachment(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
                    .await
                    .is_err()
            );
            assert!(client.execute(finish(&client, &upload)).await.is_err());
        }
        let upload = prepare(&client, spec(fixture.worktree, "image", b"valid")).await;
        stage(&client, &upload, b"valid").await;
        let attachment = publish(&client, &upload).await;
        let path = fixture
            .node
            .profile()
            .join("attachments")
            .join(attachment.id.to_string());
        std::fs::write(path, b"other").unwrap();
        let Output::AttachmentDownload(download) = client
            .execute(client.prepare(Command::DownloadAttachment {
                worktree: fixture.worktree,
                attachment: attachment.id,
            }))
            .await
            .unwrap()
        else {
            panic!("download expected")
        };
        assert_eq!(
            client
                .download_attachment(&download, &mut Vec::new(), CancellationToken::new(), |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    fixture.close().await;
}
