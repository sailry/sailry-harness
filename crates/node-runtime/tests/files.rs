use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn worktree(client: &Client, path: &std::path::Path) -> WorktreeId {
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Fixture".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    snapshot.worktrees[0].id
}

async fn read(client: &Client, worktree: WorktreeId, path: &str) -> Result<Output, Fault> {
    client
        .execute(client.prepare(Command::ReadFile {
            worktree,
            path: path.into(),
        }))
        .await
}

#[tokio::test]
async fn shares_read_only_worktree_access() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(project.join("README.md"), "# Fixture\n").unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invite = node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    for client in [&local, &remote] {
        let Output::Directory(directory) = client
            .execute(client.prepare(Command::ListDirectory {
                worktree: id,
                path: String::new(),
                after: None,
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            directory
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["src", "README.md"]
        );
        assert_eq!(directory.entries[0].kind, EntryKind::Directory);
        assert!(!directory.truncated);
        let Output::FileContent(content) = read(client, id, "src/main.rs").await.unwrap() else {
            panic!()
        };
        assert_eq!(content.text, "fn main() {}\n");
        assert_eq!(
            content.revision,
            Some(blake3::hash(content.text.as_bytes()).to_hex().to_string())
        );
        assert!(!content.truncated);
        assert_eq!(
            read(client, WorktreeId::new(), "README.md")
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        for path in [
            "../README.md",
            "/etc/passwd",
            "src/../README.md",
            "src//main.rs",
            "src\\main.rs",
            "C:/data",
            "",
            "src/./main.rs",
            "src/main.rs\0",
        ] {
            assert_eq!(
                read(client, id, path).await.unwrap_err().code,
                ErrorCode::InvalidRequest,
                "{path:?}"
            );
        }
    }
    let request = remote.prepare(Command::ReadFile {
        worktree: id,
        path: "README.md".into(),
    });
    let admission = transport.dispatch(request).await.unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "only project registration is durable");
    drop(db);
    node.link()
        .set_trust(NodeId(*controller.handle().address().id.as_bytes()), false)
        .await
        .unwrap();
    assert!(read(&remote, id, "README.md").await.is_err());
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    assert!(read(&local, id, "README.md").await.is_err());
}

#[tokio::test]
async fn bounds_utf8_previews() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &project).await;
    let text = "€".repeat(MAX_FILE_BYTES);
    std::fs::write(project.join("large.txt"), &text).unwrap();
    let Output::FileContent(content) = read(&client, id, "large.txt").await.unwrap() else {
        panic!()
    };
    assert!(content.truncated);
    assert!(content.revision.is_none());
    assert_eq!(content.text.len(), MAX_FILE_BYTES - MAX_FILE_BYTES % 3);
    assert_eq!(content.size, text.len() as u64);
    for (name, bytes) in [("invalid.bin", &[255u8][..]), ("nul.bin", &b"a\0b"[..])] {
        std::fs::write(project.join(name), bytes).unwrap();
        assert_eq!(
            read(&client, id, name).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    for index in 0..MAX_DIRECTORY_ENTRIES {
        std::fs::write(project.join(format!("{index}.txt")), "").unwrap();
    }
    let Output::Directory(directory) = client
        .execute(client.prepare(Command::ListDirectory {
            worktree: id,
            path: String::new(),
            after: None,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(directory.truncated);
    assert_eq!(directory.entries.len(), MAX_DIRECTORY_ENTRIES);
    node.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn confines_links_and_special_files() {
    use std::{ffi::CString, os::unix::fs::symlink};
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(temp.path().join("secret.txt"), "private").unwrap();
    symlink(temp.path(), project.join("outside")).unwrap();
    symlink(temp.path().join("secret.txt"), project.join("secret.txt")).unwrap();
    std::fs::write(project.join("not:portable"), "unsupported").unwrap();
    let fifo = CString::new(project.join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &project).await;
    for path in ["outside/secret.txt", "secret.txt", "pipe"] {
        assert!(read(&client, id, path).await.is_err(), "{path}");
    }
    let Output::Directory(directory) = client
        .execute(client.prepare(Command::ListDirectory {
            worktree: id,
            path: String::new(),
            after: None,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(directory.unsupported_names, 1);
    assert!(
        directory
            .entries
            .iter()
            .any(|entry| entry.name == "outside" && entry.kind == EntryKind::Symlink)
    );
    // Replacing the registered root itself must not cause ambient symlink traversal.
    std::fs::rename(&project, temp.path().join("original")).unwrap();
    symlink(temp.path(), &project).unwrap();
    assert!(read(&client, id, "secret.txt").await.is_err());
    node.shutdown().await.unwrap();
}
