use std::path::Path;

use git2::{Repository, Signature};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[path = "git/history.rs"]
mod history;

#[path = "git/reads.rs"]
mod reads;

#[path = "git/branches.rs"]
mod branches;

#[path = "git/checkout.rs"]
mod checkout;

#[path = "git/removal.rs"]
mod removal;

#[path = "git/rename.rs"]
mod rename;

#[path = "git/revision.rs"]
mod revision;

#[path = "git/merge.rs"]
mod merge;

#[path = "git/initialization.rs"]
mod initialization;

async fn worktree(client: &Client, path: &Path) -> WorktreeId {
    let Output::Project(project) = client
        .execute(client.prepare(Command::RegisterProject {
            name: "Git fixture".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.project == Some(project.id))
        .unwrap()
        .id
}

async fn status(client: &Client, worktree: WorktreeId) -> GitStatus {
    let Output::GitStatus(status) = client
        .execute(client.prepare(Command::InspectGit { worktree }))
        .await
        .unwrap()
    else {
        panic!()
    };
    status
}

async fn diff(client: &Client, worktree: WorktreeId, path: &str, scope: GitDiffScope) -> GitDiff {
    let Output::GitDiff(diff) = client
        .execute(client.prepare(Command::ReadGitDiff {
            worktree,
            path: path.into(),
            scope,
        }))
        .await
        .unwrap()
    else {
        panic!()
    };
    diff
}

fn commit(repository: &Repository) {
    let mut index = repository.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
}

#[tokio::test]
async fn shares_staged_and_unstaged_reads() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("README.md"), "base\n").unwrap();
    std::fs::write(root.join("removed.txt"), "removed\n").unwrap();
    commit(&repository);
    std::fs::write(root.join("README.md"), "staged\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("README.md")).unwrap();
    index.write().unwrap();
    std::fs::write(root.join("README.md"), "working\n").unwrap();
    std::fs::remove_file(root.join("removed.txt")).unwrap();
    std::fs::write(root.join("中文[1].txt"), "你好\n").unwrap();
    std::fs::write(root.join("中文1.txt"), "not the selected file\n").unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &root).await;
    let controller = Link::controller(temp.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    for client in [&local, &remote] {
        let status = status(client, id).await;
        assert_eq!(status.kind, RepositoryKind::Ready);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.entries.len(), 4);
        assert!(!status.truncated);
        let readme = status
            .entries
            .iter()
            .find(|entry| entry.path == "README.md")
            .unwrap();
        assert_eq!(readme.staged, Some(GitChangeKind::Modified));
        assert_eq!(readme.unstaged, Some(GitChangeKind::Modified));
        for (scope, old, new) in [
            (GitDiffScope::Staged, "base", "staged"),
            (GitDiffScope::Unstaged, "staged", "working"),
            (GitDiffScope::All, "base", "working"),
        ] {
            let patch = diff(client, id, "README.md", scope).await;
            assert!(patch.text.contains(&format!("-{old}\n")));
            assert!(patch.text.contains(&format!("+{new}\n")));
            assert_eq!((patch.additions, patch.deletions), (1, 1));
            assert!(!patch.truncated);
        }
        let patch = diff(client, id, "中文[1].txt", GitDiffScope::Unstaged).await;
        assert!(patch.text.contains("+你好"));
        assert!(!patch.text.contains("not the selected"));
        for invalid in [
            "../README.md",
            ".git/config",
            "/etc/passwd",
            "a/../b",
            "a\\b",
            "a\n",
            "",
        ] {
            let error = client
                .execute(client.prepare(Command::ReadGitDiff {
                    worktree: id,
                    path: invalid.into(),
                    scope: GitDiffScope::All,
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
        }
    }
    let admission = transport
        .dispatch(remote.prepare(Command::InspectGit { worktree: id }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    let database = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(database);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn handles_repository_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    assert_eq!(status(&client, id).await.kind, RepositoryKind::Directory);
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let initial = status(&client, id).await;
    assert_eq!(initial.kind, RepositoryKind::Unborn);
    assert_eq!(initial.branch.as_deref(), Some("main"));
    std::fs::write(root.join("binary"), b"a\0b").unwrap();
    std::fs::write(root.join("large"), "一二三\n".repeat(10_000)).unwrap();
    assert!(diff(&client, id, "binary", GitDiffScope::All).await.binary);
    let large = diff(&client, id, "large", GitDiffScope::All).await;
    assert!(large.truncated);
    assert!(large.text.len() <= MAX_DIFF_BYTES);
    std::fs::create_dir(root.join("child")).unwrap();
    let child = worktree(&client, &root.join("child")).await;
    assert_eq!(
        status(&client, child).await.kind,
        RepositoryKind::Directory,
        "must not discover a parent repository"
    );
    for index in 0..MAX_GIT_ENTRIES {
        std::fs::write(root.join(format!("file-{index}")), "").unwrap();
    }
    let listing = status(&client, id).await;
    assert!(listing.truncated);
    assert_eq!(listing.entries.len(), MAX_GIT_ENTRIES);
    node.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn confines_repository_reads() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("private"), "external secret sentinel").unwrap();
    let repository = Repository::init(&root).unwrap();
    symlink(outside.join("private"), root.join("link")).unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    std::fs::write(root.join(".gitattributes"), "*.txt filter=fixture\n").unwrap();
    std::fs::write(root.join("file.txt"), "safe\n").unwrap();
    let marker = temp.path().join("filter-ran");
    repository
        .config()
        .unwrap()
        .set_str(
            "filter.fixture.clean",
            &format!("touch {}", marker.display()),
        )
        .unwrap();
    let pipe = std::ffi::CString::new(root.join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    let id = worktree(&client, &root).await;
    let listing = status(&client, id).await;
    assert!(listing.omitted_paths > 0);
    assert!(
        !listing
            .entries
            .iter()
            .any(|entry| entry.path.starts_with("escape/"))
    );
    let link = diff(&client, id, "link", GitDiffScope::All).await;
    assert!(!link.text.contains("external secret sentinel"));
    let _ = diff(&client, id, "file.txt", GitDiffScope::All).await;
    assert!(!marker.exists());
    for path in ["escape/private", "pipe"] {
        assert!(
            client
                .execute(client.prepare(Command::ReadGitDiff {
                    worktree: id,
                    path: path.into(),
                    scope: GitDiffScope::All
                }))
                .await
                .is_err()
        );
    }
    std::fs::rename(&root, temp.path().join("moved")).unwrap();
    symlink(&outside, &root).unwrap();
    assert!(
        client
            .execute(client.prepare(Command::InspectGit { worktree: id }))
            .await
            .is_err()
    );
    node.shutdown().await.unwrap();
}
