use super::*;
use sailry_protocol::{Command, ErrorCode, Output, attachment};
use std::time::Duration;

#[tokio::test]
async fn oversize_preserves_digest() {
    let digest = Digest::new();
    let bytes = "附件 🙂".as_bytes();
    digest.update(bytes.to_vec()).await.unwrap();
    assert!(
        digest
            .update(vec![0; FILE_TRANSFER_CHUNK_BYTES + 1])
            .await
            .is_err()
    );
    digest.update(bytes.to_vec()).await.unwrap();
    assert_eq!(
        digest.revision().await,
        blake3::hash(&bytes.repeat(2)).to_hex().to_string()
    );
}

mod download {
    use super::*;

    #[tokio::test]
    async fn partial_bytes_do_not_imply_success() {
        let (mut writer, reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let download = Download {
            reader: Mutex::new(reader),
            transfer: Transfer::spawn(CancellationToken::new(), async move {
                writer.write_all(b"partial").await.unwrap();
                Err(Fault::new(ErrorCode::RevisionConflict, "digest mismatch"))
            }),
        };
        assert_eq!(download.next().await.unwrap().unwrap(), b"partial");
        for _ in 0..2 {
            assert!(
                download
                    .next()
                    .await
                    .unwrap_err()
                    .contains("RevisionConflict")
            );
        }
    }

    #[tokio::test]
    async fn closing_interrupts_a_waiting_read() {
        let (writer, reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let download = Download {
            reader: Mutex::new(reader),
            transfer: Transfer::spawn(stop, async move {
                cancellation.cancelled().await;
                drop(writer);
                Err(Fault::new(ErrorCode::Cancelled, "cancelled"))
            }),
        };
        let next = download.next();
        tokio::pin!(next);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut next)
                .await
                .is_err()
        );
        download.close();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), next)
                .await
                .unwrap()
                .is_err()
        );
    }
}

mod upload {
    use super::*;

    #[tokio::test]
    async fn closing_interrupts_backpressure() {
        let (writer, reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let upload = Upload {
            writer: Mutex::new(Some(writer)),
            transfer: Transfer::spawn(stop, async move {
                cancellation.cancelled().await;
                drop(reader);
                Err(Fault::new(ErrorCode::Cancelled, "cancelled"))
            }),
        };
        upload
            .write(vec![0; FILE_TRANSFER_CHUNK_BYTES])
            .await
            .unwrap();
        let write = upload.write(vec![1]);
        tokio::pin!(write);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut write)
                .await
                .is_err()
        );
        upload.close();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), write)
                .await
                .unwrap()
                .is_err()
        );
        assert!(upload.finish().await.is_err());
    }

    #[tokio::test]
    async fn drop_releases_client() {
        let (writer, reader) = tokio::io::duplex(FILE_TRANSFER_CHUNK_BYTES);
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let (cleaned, cleanup) = tokio::sync::oneshot::channel();
        let upload = Upload {
            writer: Mutex::new(Some(writer)),
            transfer: Transfer::spawn(stop, async move {
                cancellation.cancelled().await;
                drop(reader);
                cleaned.send(()).unwrap();
                Err(Fault::new(ErrorCode::Cancelled, "cancelled"))
            }),
        };
        drop(upload);
        tokio::time::timeout(Duration::from_secs(1), cleanup)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn local_roundtrip_validates_digest() {
    let directory = tempfile::tempdir().unwrap();
    let node = sailry_node_runtime::Node::start(directory.path().join("node"))
        .await
        .unwrap();
    let connection = Connection::new(node.local(), CancellationToken::new(), Default::default());
    let Output::Project(project) = execute(
        &connection,
        Command::RegisterProject {
            name: "FFI attachments".into(),
            path: directory.path().to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = execute(&connection, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id))
        .unwrap()
        .id;
    let image = b"image bytes";
    std::fs::write(directory.path().join("image.png"), image).unwrap();
    let Output::FileDownload(descriptor) = execute(
        &connection,
        Command::DownloadFile {
            worktree,
            path: "image.png".into(),
        },
    )
    .await
    else {
        panic!("file download expected")
    };
    let download = connection
        .download_file(serde_json::to_string(&descriptor).unwrap())
        .await
        .unwrap();
    assert_eq!(download.next().await.unwrap().unwrap(), image);
    assert!(download.next().await.unwrap().is_none());
    for bytes in [
        Vec::new(),
        (0..4 * 1024 * 1024 + 17).map(|index| index as u8).collect(),
    ] {
        let spec = attachment::Spec {
            worktree,
            name: "附件 🙂.bin".into(),
            media_type: "application/octet-stream".into(),
            size: bytes.len() as u64,
            revision: blake3::hash(&bytes).to_hex().to_string(),
        };
        let Output::AttachmentUpload(descriptor) =
            execute(&connection, Command::UploadAttachment(spec)).await
        else {
            panic!("upload expected")
        };
        let upload = connection
            .upload_attachment(serde_json::to_string(&descriptor).unwrap())
            .await
            .unwrap();
        for chunk in bytes.chunks(FILE_TRANSFER_CHUNK_BYTES) {
            upload.write(chunk.to_vec()).await.unwrap();
        }
        upload.finish().await.unwrap();
        upload.finish().await.unwrap();
        assert!(upload.write(vec![0]).await.is_err());
        let Output::Attachment(attachment) = execute(
            &connection,
            Command::FinishAttachmentUpload {
                worktree,
                stream: descriptor.stream,
            },
        )
        .await
        else {
            panic!("attachment expected")
        };
        for valid in [true, false] {
            let Output::AttachmentDownload(mut descriptor) = execute(
                &connection,
                Command::DownloadAttachment {
                    worktree,
                    attachment: attachment.id,
                },
            )
            .await
            else {
                panic!("download expected")
            };
            if !valid {
                descriptor.attachment.spec.revision = "00".repeat(32);
            }
            let download = connection
                .download_attachment(serde_json::to_string(&descriptor).unwrap())
                .await
                .unwrap();
            let mut received = Vec::new();
            loop {
                match download.next().await {
                    Ok(Some(chunk)) => {
                        assert!(chunk.len() <= FILE_TRANSFER_CHUNK_BYTES);
                        received.extend(chunk);
                    }
                    Ok(None) => {
                        assert!(valid);
                        break;
                    }
                    Err(failure) => {
                        assert!(!valid);
                        assert!(failure.contains("RevisionConflict"));
                        break;
                    }
                }
            }
            assert_eq!(received, bytes);
        }
    }
    connection.close();
    node.shutdown().await.unwrap();
}

async fn execute(connection: &Connection, command: Command) -> Output {
    let request = connection
        .prepare(serde_json::to_string(&command).unwrap())
        .unwrap();
    serde_json::from_str::<Result<Output, Fault>>(&connection.execute(request).await.unwrap())
        .unwrap()
        .unwrap()
}
