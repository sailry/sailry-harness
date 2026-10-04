use sailry_client::Client;
use sailry_link::{CancellationToken, Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::PathBuf, sync::Arc};

#[cfg(target_os = "macos")]
#[path = "support/trash.rs"]
mod trash_fixture;

#[path = "file_transfer/failures.rs"]
mod failures;
#[path = "file_transfer/moving.rs"]
mod moving;
#[path = "file_transfer/recovery.rs"]
mod recovery;

const PATHS: [(usize, usize); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];

struct Endpoint {
    root: PathBuf,
    profile: PathBuf,
    node: Node,
    transports: [Arc<dyn Transport>; 2],
    worktree: WorktreeId,
}

impl Endpoint {
    async fn start(path: PathBuf, controller: &Link) -> Self {
        let root = path.join("project");
        let profile = path.join("node");
        std::fs::create_dir_all(&root).unwrap();
        let node = Node::start(&profile).await.unwrap();
        let local = node.local();
        let client = Client::new(local.clone());
        client
            .execute(client.prepare(Command::RegisterProject {
                name: "Transfer fixture".into(),
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
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        Self {
            root,
            profile,
            node,
            transports: [local, controller.handle().remote(address)],
            worktree: snapshot.worktrees[0].id,
        }
    }

    fn count(&self) -> i64 {
        rusqlite::Connection::open(self.profile.join("storage/node.sqlite3"))
            .unwrap()
            .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
            .unwrap()
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    controller: Link,
    source: Endpoint,
    destination: Endpoint,
}

impl Fixture {
    async fn start() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let source = Endpoint::start(temp.path().join("source"), &controller).await;
        let destination = Endpoint::start(temp.path().join("destination"), &controller).await;
        Self {
            _temp: temp,
            controller,
            source,
            destination,
        }
    }

    fn clients(&self, (source, target): (usize, usize)) -> (Client, Client) {
        (
            Client::new(self.source.transports[source].clone()),
            Client::new(self.destination.transports[target].clone()),
        )
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.source.node.shutdown().await.unwrap();
        self.destination.node.shutdown().await.unwrap();
    }
}

async fn prepare(client: &Client, worktree: WorktreeId, path: &str) -> FileDownload {
    let Output::FileDownload(download) = client
        .execute(client.prepare(Command::DownloadFile {
            worktree,
            path: path.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("download expected")
    };
    assert_eq!(download.worktree, worktree);
    assert_eq!(download.path, path);
    download
}

fn spec(download: &FileDownload, worktree: WorktreeId, path: &str) -> FileUploadSpec {
    FileUploadSpec {
        worktree,
        path: path.into(),
        size: download.size,
        revision: download.revision.clone(),
        expected_revision: None,
    }
}

#[tokio::test]
async fn waits_for_publication() {
    let fixture = Fixture::start().await;
    let data: Vec<_> = (0..10 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    for (path, bytes) in [("资料.bin", data.as_slice()), ("empty", &[])] {
        std::fs::write(fixture.source.root.join(path), bytes).unwrap();
        for pair in PATHS {
            let (source, target) = fixture.clients(pair);
            assert_ne!(source.target(), target.target());
            let counts = (fixture.source.count(), fixture.destination.count());
            let download = prepare(&source, fixture.source.worktree, path).await;
            let destination = format!("{}-{}-{path}", pair.0, pair.1);
            let mut progress = Vec::new();
            let request = target
                .stage_copy(
                    &source,
                    &download,
                    spec(&download, fixture.destination.worktree, &destination),
                    CancellationToken::new(),
                    |bytes| progress.push(bytes),
                )
                .await
                .unwrap();
            assert_eq!(progress[0], 0);
            assert_eq!(*progress.last().unwrap(), bytes.len() as u64);
            assert!(
                progress.windows(2).all(|pair| pair[0] < pair[1]
                    && pair[1] - pair[0] <= FILE_TRANSFER_CHUNK_BYTES as u64)
            );
            assert_eq!(fixture.source.count(), counts.0);
            assert_eq!(fixture.destination.count(), counts.1);
            assert!(!fixture.destination.root.join(&destination).exists());
            assert_eq!(
                std::fs::read(fixture.source.root.join(path)).unwrap(),
                bytes
            );
            assert!(serde_json::to_vec(&request).unwrap().len() < 1024);
            assert_eq!(
                source.execute(request.clone()).await.unwrap_err().code,
                ErrorCode::WrongTarget
            );
            let mut changes = fixture.destination.transports[pair.1]
                .subscribe(Topic::Files(fixture.destination.worktree))
                .await
                .unwrap();
            changes.next().await.unwrap();
            let admission = target.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let output = admission.completion.await.unwrap().unwrap();
            assert_eq!(
                output,
                Output::FileWritten(FileWritten {
                    path: destination.clone(),
                    size: bytes.len() as u64,
                    revision: download.revision,
                })
            );
            assert_eq!(fixture.source.count(), counts.0);
            assert_eq!(fixture.destination.count(), counts.1 + 1);
            assert_eq!(
                std::fs::read(fixture.destination.root.join(&destination)).unwrap(),
                bytes
            );
            tokio::time::timeout(std::time::Duration::from_secs(3), changes.next())
                .await
                .unwrap()
                .unwrap();
            std::fs::write(fixture.destination.root.join(&destination), "external").unwrap();
            assert_eq!(target.execute(request).await.unwrap(), output);
            assert_eq!(
                std::fs::read(fixture.destination.root.join(destination)).unwrap(),
                b"external"
            );
        }
    }
    fixture.close().await;
}
