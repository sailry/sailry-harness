use super::*;
use std::os::unix::fs::PermissionsExt;

struct Denied {
    path: std::path::PathBuf,
    permissions: std::fs::Permissions,
}

impl Denied {
    fn new(path: &Path) -> Option<Self> {
        Self::with_mode(path, 0o000)
    }

    fn with_mode(path: &Path, mode: u32) -> Option<Self> {
        let denied = Self {
            path: path.to_owned(),
            permissions: std::fs::metadata(path).unwrap().permissions(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        // Privileged runners can bypass mode bits and cannot exercise this fixture.
        if std::fs::read_dir(path).is_ok() {
            return None;
        }
        Some(denied)
    }
}

impl Drop for Denied {
    fn drop(&mut self) {
        std::fs::set_permissions(&self.path, self.permissions.clone())
            .expect("restore fixture permissions");
    }
}

#[tokio::test]
async fn skips_inaccessible_descendants() {
    inaccessible(0o000, false).await;
}

#[tokio::test]
async fn skips_ignored_tracked_directories() {
    inaccessible(0o111, true).await;
}

async fn inaccessible(mode: u32, ignored: bool) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    std::fs::create_dir(root.join("locked2")).unwrap();
    std::fs::create_dir(root.join("untracked")).unwrap();
    std::fs::write(root.join("locked/tracked.txt"), "tracked\n").unwrap();
    std::fs::write(root.join("locked2/sibling.txt"), "original\n").unwrap();
    std::fs::write(root.join("tracked.txt"), "original\n").unwrap();
    commit(&repository);
    if ignored {
        std::fs::write(root.join(".gitignore"), "locked/\n").unwrap();
    }
    std::fs::write(root.join("locked/tracked.txt"), "inaccessible change\n").unwrap();
    std::fs::write(root.join("locked2/sibling.txt"), "sibling change\n").unwrap();
    std::fs::write(root.join("tracked.txt"), "visible change\n").unwrap();
    std::fs::write(root.join("visible.txt"), "untracked change\n").unwrap();
    std::fs::write(root.join("untracked/hidden.txt"), "inaccessible\n").unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let local = Client::new(node.local());
    let worktree = worktree(&local, &root).await;
    let controller = Link::controller(directory.path().join("client"), NetworkScope::default())
        .await
        .unwrap();
    let peer = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(peer));
    let Some(_tracked) = Denied::with_mode(&root.join("locked"), mode) else {
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        return;
    };
    let Some(_untracked) = Denied::new(&root.join("untracked")) else {
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        return;
    };
    for client in [&local, &remote] {
        let status = status(client, worktree).await;
        assert_eq!(status.kind, RepositoryKind::Ready);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert!(!status.truncated);
        assert_eq!(status.omitted_paths, 0);
        assert_eq!(status.entries.len(), 3 + usize::from(ignored));
        for path in ["locked2/sibling.txt", "tracked.txt"] {
            let entry = status
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .unwrap();
            assert_eq!(entry.unstaged, Some(GitChangeKind::Modified));
            assert_eq!(entry.unstaged_diff.additions, 1);
            assert_eq!(entry.unstaged_diff.deletions, 1);
        }
        assert!(
            status
                .entries
                .iter()
                .any(|entry| entry.path == "visible.txt" && entry.untracked)
        );
        if ignored {
            assert!(
                status
                    .entries
                    .iter()
                    .any(|entry| entry.path == ".gitignore" && entry.untracked)
            );
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
