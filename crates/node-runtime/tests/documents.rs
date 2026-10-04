use sailry_client::{Client, MAX_DOCUMENT_BYTES};
use sailry_link::{CancellationToken, Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::*;
use std::{path::PathBuf, sync::Arc};

#[path = "documents/failures.rs"]
mod failures;

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
                name: "Document fixture".into(),
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
            worktree: snapshot.worktrees[0].id,
        }
    }

    fn count(&self) -> i64 {
        rusqlite::Connection::open(self.profile.join("storage/node.sqlite3"))
            .unwrap()
            .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
            .unwrap()
    }

    async fn close(self) {
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shares_revision_checked_publication() {
    let fixture = Fixture::start().await;
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        for size in [
            0,
            MAX_FILE_BYTES,
            MAX_FILE_BYTES + 1,
            MAX_DOCUMENT_BYTES,
            MAX_DOCUMENT_BYTES + 1,
        ] {
            let path = format!("资料-{index}-{size}.txt");
            // A multibyte character straddles the preview boundary.
            let text = if size > MAX_FILE_BYTES {
                format!(
                    "{}资{}",
                    "a".repeat(MAX_FILE_BYTES - 2),
                    "z".repeat(size - MAX_FILE_BYTES - 1)
                )
            } else {
                "a".repeat(size)
            };
            std::fs::write(fixture.root.join(&path), &text).unwrap();
            let before = fixture.count();
            let content = client
                .read_document(fixture.worktree, &path, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(fixture.count(), before);
            if text.len() > MAX_DOCUMENT_BYTES {
                assert!(content.truncated);
                assert!(content.revision.is_none());
                assert!(content.text.len() <= MAX_FILE_BYTES);
                continue;
            }
            assert_eq!(content.text, text);
            assert!(!content.truncated);
            let expected = content.revision.unwrap();
            // JSON would exceed the control-frame limit if these bytes were inline.
            let next = "\u{0001}".repeat(MAX_DOCUMENT_BYTES);
            let staged = client
                .stage_document(
                    fixture.worktree,
                    &path,
                    &next,
                    &expected,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert!(staged.matches(fixture.worktree, &path, &next, &expected));
            assert_eq!(fixture.count(), before);
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
                text
            );
            assert!(serde_json::to_vec(&staged.request).unwrap().len() < 1024);
            assert!(matches!(
                staged.request.command,
                Command::FinishFileUpload { .. }
            ));
            let result = client.execute(staged.request.clone()).await.unwrap();
            let Output::FileWritten(written) = &result else {
                panic!("written result expected")
            };
            assert_eq!(written.size, next.len() as u64);
            assert_eq!(
                written.revision,
                blake3::hash(next.as_bytes()).to_hex().to_string()
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
                next
            );
            let read = client
                .read_document(fixture.worktree, &path, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(read.text, next);
            assert_eq!(read.revision.as_deref(), Some(written.revision.as_str()));
            let shrinking = client
                .stage_document(
                    fixture.worktree,
                    &path,
                    "",
                    &written.revision,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            client.execute(shrinking.request).await.unwrap();
            assert!(std::fs::read(fixture.root.join(&path)).unwrap().is_empty());
            std::fs::write(fixture.root.join(&path), "external").unwrap();
            assert_eq!(client.execute(staged.request).await.unwrap(), result);
            assert_eq!(
                std::fs::read_to_string(fixture.root.join(&path)).unwrap(),
                "external"
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn rejects_incomplete_staging() {
    let fixture = Fixture::start().await;
    for transport in &fixture.transports {
        let client = Client::new(transport.clone());
        let path = "unchanged.txt";
        std::fs::write(fixture.root.join(path), "original").unwrap();
        let expected = blake3::hash(b"original").to_hex().to_string();
        let next = "text\n".repeat(40_000);
        let before = fixture.count();
        let stop = CancellationToken::new();
        stop.cancel();
        assert_eq!(
            client
                .read_document(fixture.worktree, path, stop.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            client
                .stage_document(fixture.worktree, path, &next, &expected, stop)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        for text in ["x".repeat(MAX_DOCUMENT_BYTES + 1), "binary\0data".into()] {
            assert_eq!(
                client
                    .stage_document(
                        fixture.worktree,
                        path,
                        &text,
                        &expected,
                        CancellationToken::new()
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(fixture.count(), before);
        let staged = client
            .stage_document(
                fixture.worktree,
                path,
                &next,
                &expected,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        std::fs::write(fixture.root.join(path), "changed before publication").unwrap();
        assert_eq!(
            client
                .execute(staged.request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client.execute(staged.request).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(path)).unwrap(),
            "changed before publication"
        );
        let mut binary = vec![b'a'; MAX_FILE_BYTES + 50];
        binary[MAX_FILE_BYTES + 10] = 0;
        std::fs::write(fixture.root.join(path), &binary).unwrap();
        assert_eq!(
            client
                .read_document(fixture.worktree, path, CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        binary[MAX_FILE_BYTES + 10] = 0xff;
        std::fs::write(fixture.root.join(path), &binary).unwrap();
        assert_eq!(
            client
                .read_document(fixture.worktree, path, CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    fixture.close().await;
}
