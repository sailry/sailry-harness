use sailry_client::Client;
use sailry_link::{CancellationToken, Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::PathBuf, sync::Arc, time::Duration};

#[path = "file_upload/guards.rs"]
mod guards;
#[path = "file_upload/lifecycle.rs"]
mod lifecycle;
#[path = "file_upload/recovery.rs"]
mod recovery;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    profile: PathBuf,
    node: Node,
    controller: Link,
    transports: [Arc<dyn Transport>; 2],
    worktree: WorktreeId,
}

impl Fixture {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let profile = temp.path().join("node");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(&profile).await.unwrap();
        let local = node.local();
        let client = Client::new(local.clone());
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Upload fixture".into(),
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
        let worktree = snapshot.worktrees[0].id;
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let remote = controller.handle().remote(address);
        Self {
            _temp: temp,
            root,
            profile,
            node,
            controller,
            transports: [local, remote],
            worktree,
        }
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

fn spec(worktree: WorktreeId, path: &str, data: &[u8], expected: Option<&str>) -> FileUploadSpec {
    FileUploadSpec {
        worktree,
        path: path.into(),
        size: data.len() as u64,
        revision: blake3::hash(data).to_hex().to_string(),
        expected_revision: expected.map(str::to_owned),
    }
}

async fn prepare(client: &Client, spec: FileUploadSpec) -> FileUpload {
    let admission = client
        .dispatch(client.prepare(Command::UploadFile(spec.clone())))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::FileUpload(upload) = admission.completion.await.unwrap().unwrap() else {
        panic!("upload expected")
    };
    assert_eq!(upload.spec, spec);
    upload
}

fn finish(client: &Client, upload: &FileUpload) -> Request {
    client.prepare(Command::FinishFileUpload {
        worktree: upload.spec.worktree,
        path: upload.spec.path.clone(),
        stream: upload.stream,
    })
}

async fn stage(client: &Client, upload: &FileUpload, data: &[u8]) {
    let mut progress = Vec::new();
    client
        .upload(upload, &mut &data[..], CancellationToken::new(), |bytes| {
            progress.push(bytes)
        })
        .await
        .unwrap();
    assert_eq!(progress[0], 0);
    assert_eq!(*progress.last().unwrap(), data.len() as u64);
    assert!(
        progress
            .windows(2)
            .all(|pair| pair[1] > pair[0] && pair[1] - pair[0] <= FILE_TRANSFER_CHUNK_BYTES as u64)
    );
}

#[tokio::test]
async fn publishes_verified_staging() {
    let fixture = Fixture::start().await;
    let database =
        rusqlite::Connection::open(fixture.profile.join("storage/node.sqlite3")).unwrap();
    let count = || {
        database
            .query_row("SELECT count(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let data: Vec<_> = (0..10 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let other = Client::new(fixture.transports[1 - index].clone());
        for (name, data) in [("资料.bin", data.as_slice()), ("empty", &[])] {
            let path = format!("{index}-{name}");
            let before = count();
            let upload = prepare(&client, spec(fixture.worktree, &path, data, None)).await;
            assert!(other.open(upload.stream).await.is_err());
            assert_eq!(
                other
                    .execute(other.prepare(Command::CancelFileTransfer {
                        stream: upload.stream
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            stage(&client, &upload, data).await;
            assert_eq!(count(), before, "staged bytes are not durable commands");
            assert!(!fixture.root.join(&path).exists());
            assert_eq!(
                other
                    .execute(finish(&other, &upload))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let request = finish(&client, &upload);
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let output = admission.completion.await.unwrap().unwrap();
            assert_eq!(
                output,
                Output::FileWritten(FileWritten {
                    path: path.clone(),
                    revision: upload.spec.revision.clone(),
                    size: data.len() as u64
                })
            );
            assert_eq!(std::fs::read(fixture.root.join(&path)).unwrap(), data);
            // Repeated completion queries cannot replace a later external edit.
            std::fs::write(fixture.root.join(&path), "external").unwrap();
            assert_eq!(client.execute(request.clone()).await.unwrap(), output);
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
                "external"
            );
            let mut conflicting = request;
            conflicting.command = Command::FinishFileUpload {
                worktree: fixture.worktree,
                path: "other".into(),
                stream: upload.stream,
            };
            assert_eq!(
                client.execute(conflicting).await.unwrap_err().code,
                ErrorCode::Conflict
            );

            let expected = blake3::hash(b"external").to_hex().to_string();
            let replacement = prepare(
                &client,
                spec(fixture.worktree, &path, data, Some(&expected)),
            )
            .await;
            stage(&client, &replacement, data).await;
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
                "external"
            );
            client.execute(finish(&client, &replacement)).await.unwrap();
            assert_eq!(std::fs::read(fixture.root.join(&path)).unwrap(), data);
        }
    }
    drop(database);
    fixture.close().await;
}

#[tokio::test]
async fn rejects_cancelled_or_changed_inputs() {
    let fixture = Fixture::start().await;
    let data = vec![0xf1; 2 * 1024 * 1024];
    for transport in &fixture.transports {
        let client = Client::new(transport.clone());
        for mode in ["cancel", "short", "changed", "long"] {
            let upload = prepare(&client, spec(fixture.worktree, "file", &data, None)).await;
            let cancel = CancellationToken::new();
            let stop = cancel.clone();
            let changed = vec![0x11; data.len()];
            let long = vec![0xf1; data.len() + 1];
            let bytes = match mode {
                "short" => &data[..10],
                "changed" => &changed,
                "long" => &long,
                _ => &data,
            };
            let result = client
                .upload(&upload, &mut &bytes[..], cancel, |bytes| {
                    if mode == "cancel" && bytes > 0 {
                        stop.cancel();
                    }
                })
                .await;
            assert_eq!(
                result.unwrap_err().code,
                if mode == "cancel" {
                    ErrorCode::Cancelled
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            assert!(!fixture.root.join("file").exists());
            assert!(client.execute(finish(&client, &upload)).await.is_err());
        }
        let pending = prepare(&client, spec(fixture.worktree, "file", &data, None)).await;
        let stream = client.open(pending.stream).await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            client.execute(client.prepare(Command::Snapshot)),
        )
        .await
        .unwrap()
        .unwrap();
        client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: pending.stream,
            }))
            .await
            .unwrap();
        drop(stream);
        let upload = prepare(&client, spec(fixture.worktree, "file", b"ours", None)).await;
        stage(&client, &upload, b"ours").await;
        std::fs::write(fixture.root.join("file"), "external").unwrap();
        assert_eq!(
            client
                .execute(finish(&client, &upload))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("file")).unwrap(),
            "external"
        );
        std::fs::remove_file(fixture.root.join("file")).unwrap();
    }
    fixture.close().await;
}
