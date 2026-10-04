use sailry_client::Client;
use sailry_link::{CancellationToken, Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn register(client: &Client, root: &Path) -> WorktreeId {
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Download fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot.worktrees[0].id
}

async fn prepare(client: &Client, worktree: WorktreeId, path: &str) -> FileDownload {
    let admission = client
        .dispatch(client.prepare(Command::DownloadFile {
            worktree,
            path: path.into(),
        }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::FileDownload(download) = admission.completion.await.unwrap().unwrap() else {
        panic!("download expected")
    };
    assert_eq!(download.worktree, worktree);
    assert_eq!(download.path, path);
    download
}

async fn cancel(client: &Client, stream: StreamId) {
    assert_eq!(
        client
            .execute(client.prepare(Command::CancelFileTransfer { stream }))
            .await
            .unwrap(),
        Output::FileTransferCancelled { stream }
    );
}

#[tokio::test]
async fn shares_binary_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let data: Vec<_> = (0..10 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(root.join("资料.bin"), &data).unwrap();
    std::fs::write(root.join("empty"), []).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    let count = || {
        database
            .query_row("SELECT count(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before = count();
    for (client, other) in [(&local, &remote), (&remote, &local)] {
        for name in ["资料.bin", "empty"] {
            let prepared = prepare(client, worktree, name).await;
            assert!(matches!(
                other.open(prepared.stream).await,
                Err(Fault {
                    code: ErrorCode::NotFound,
                    ..
                })
            ));
            assert_eq!(
                other
                    .execute(other.prepare(Command::CancelFileTransfer {
                        stream: prepared.stream
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let mut output = Vec::new();
            let mut progress = Vec::new();
            client
                .download(&prepared, &mut output, CancellationToken::new(), |bytes| {
                    progress.push(bytes)
                })
                .await
                .unwrap();
            assert_eq!(output, if name == "empty" { &[][..] } else { &data });
            assert_eq!(progress[0], 0);
            assert_eq!(*progress.last().unwrap(), prepared.size);
            assert!(
                progress.windows(2).all(|pair| pair[0] < pair[1]
                    && pair[1] - pair[0] <= FILE_TRANSFER_CHUNK_BYTES as u64)
            );
            assert!(client.open(prepared.stream).await.is_err());
        }
        let pending = prepare(client, worktree, "资料.bin").await;
        cancel(client, pending.stream).await;
        cancel(client, pending.stream).await;
        assert!(client.open(pending.stream).await.is_err());
        let active = prepare(client, worktree, "资料.bin").await;
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let mut partial = Vec::new();
        assert_eq!(
            client
                .download(&active, &mut partial, stop, |bytes| {
                    if bytes > 0 {
                        stopping.cancel();
                    }
                })
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(!partial.is_empty() && partial.len() < data.len());
        assert!(client.open(active.stream).await.is_err());
        let failing = prepare(client, worktree, "资料.bin").await;
        let (mut writer, reader) = tokio::io::duplex(1);
        drop(reader);
        assert_eq!(
            client
                .download(&failing, &mut writer, CancellationToken::new(), |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        let blocked = prepare(client, worktree, "资料.bin").await;
        let (mut writer, mut reader) = tokio::io::duplex(1);
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            stopping.cancel();
        });
        assert_eq!(
            client
                .download(&blocked, &mut writer, stop, |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert_eq!(reader.read(&mut [0; 2]).await.unwrap(), 1);
        assert!(client.open(blocked.stream).await.is_err());
        // Control requests remain responsive while an auxiliary stream is unread.
        let waiting = prepare(client, worktree, "资料.bin").await;
        let stream = client.open(waiting.stream).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            client.execute(client.prepare(Command::Snapshot)),
        )
        .await
        .unwrap()
        .unwrap();
        cancel(client, waiting.stream).await;
        drop(stream);
    }
    assert_eq!(
        count(),
        before,
        "temporary transfers must not enter the durable ledger"
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_changed_or_unconfined_sources() {
    let temp = tempfile::tempdir().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, temp.path()).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    std::fs::write(temp.path().join("node/sentinel"), "protected").unwrap();
    for client in [&local, &remote] {
        for path in [
            "",
            "../outside",
            "missing",
            "node/sentinel",
            "node/storage/node.sqlite3",
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::DownloadFile {
                        worktree,
                        path: path.into()
                    }))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(Command::DownloadFile {
                    worktree: WorktreeId::new(),
                    path: "file".into()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut wrong = client.prepare(Command::DownloadFile {
            worktree,
            path: "file".into(),
        });
        wrong.target = NodeId([0; 32]);
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        #[cfg(unix)]
        {
            let link = temp.path().join("link");
            std::os::unix::fs::symlink(temp.path().join("node"), &link).unwrap();
            assert!(
                client
                    .execute(client.prepare(Command::DownloadFile {
                        worktree,
                        path: "link/sentinel".into()
                    }))
                    .await
                    .is_err()
            );
            std::fs::remove_file(link).unwrap();
        }
        std::fs::write(temp.path().join("changed"), "original").unwrap();
        let source = prepare(client, worktree, "changed").await;
        std::fs::write(temp.path().join("changed"), "modified").unwrap();
        let mut partial = Vec::new();
        assert_eq!(
            client
                .download(&source, &mut partial, CancellationToken::new(), |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(partial, b"modified");
        let source = prepare(client, worktree, "changed").await;
        std::fs::write(temp.path().join("changed"), "short").unwrap();
        assert_eq!(
            client
                .download(&source, &mut Vec::new(), CancellationToken::new(), |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn closes_streams_on_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), vec![7; 32 * 1024 * 1024]).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let local_file = prepare(&local, worktree, "file").await;
    let remote_file = prepare(&remote, worktree, "file").await;
    let pending = prepare(&local, worktree, "file").await;
    let mut streams = vec![
        local.open(local_file.stream).await.unwrap(),
        remote.open(remote_file.stream).await.unwrap(),
    ];
    for stream in &mut streams {
        stream.shutdown().await.unwrap();
    }
    node.shutdown().await.unwrap();
    for stream in &mut streams {
        let mut bytes = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
            .await
            .unwrap();
        assert!(bytes.len() < 32 * 1024 * 1024);
    }
    assert!(local.open(pending.stream).await.is_err());
    let restarted = Node::start(temp.path().join("node")).await.unwrap();
    let resumed = Client::new(controller.handle().remote(restarted.link().address()));
    assert!(resumed.open(pending.stream).await.is_err());
    let fresh = prepare(&resumed, worktree, "file").await;
    let mut output = Vec::new();
    resumed
        .download(&fresh, &mut output, CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(output.len(), 32 * 1024 * 1024);
    controller.close().await.unwrap();
    restarted.shutdown().await.unwrap();
}
