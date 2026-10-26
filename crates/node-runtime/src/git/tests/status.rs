use super::*;
use std::pin::Pin;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
}

fn fixture() -> (tempfile::TempDir, PathBuf, Git, Command) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    Repository::init(&root).unwrap();
    let git = Git::new(Arc::new(crate::process::Environment::ambient()));
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    (directory, root, git, command)
}

fn hold() -> (std::sync::mpsc::Sender<()>, tokio::task::JoinHandle<()>) {
    let (release, held) = std::sync::mpsc::channel();
    let task = tokio::task::spawn_blocking(move || {
        let _ = held.recv();
    });
    (release, task)
}

async fn pending(mut future: Pin<&mut impl Future>) {
    std::future::poll_fn(|context| {
        assert!(future.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
}

fn status(result: Result<Output, Fault>) -> GitStatus {
    let Output::GitStatus(status) = result.unwrap() else {
        panic!("Git status expected")
    };
    status
}

#[test]
fn shares_one_pending_scan() {
    let (_directory, root, git, command) = fixture();
    runtime().block_on(async {
        let (release, blocker) = hold();
        let mut first =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(first.as_mut()).await;
        // This second blocker separates the original scan from any redundant one.
        let (resume, separator) = hold();
        let mut second = Box::pin(git.inspect(root, command, CancellationToken::new()));
        pending(second.as_mut()).await;
        release.send(()).unwrap();
        blocker.await.unwrap();
        let original = status(first.await);
        let shared =
            std::future::poll_fn(|context| Poll::Ready(second.as_mut().poll(context))).await;
        resume.send(()).unwrap();
        separator.await.unwrap();
        let Poll::Ready(result) = shared else {
            panic!("shared scan should already be complete")
        };
        assert_eq!(status(result), original);
    });
}

#[test]
fn isolates_caller_cancellation() {
    let (_directory, root, git, command) = fixture();
    runtime().block_on(async {
        let (release, blocker) = hold();
        let stop = CancellationToken::new();
        let mut first = Box::pin(git.inspect(root.clone(), command.clone(), stop.clone()));
        let mut second =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(first.as_mut()).await;
        pending(second.as_mut()).await;
        stop.cancel();
        assert_eq!(first.await.unwrap_err().code, ErrorCode::Unavailable);
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert_eq!(status(second.await).kind, RepositoryKind::Unborn);
        // Losing the last caller also releases its join slot, without retaining cancellation.
        let (release, blocker) = hold();
        let mut abandoned =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(abandoned.as_mut()).await;
        drop(abandoned);
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert_eq!(
            status(git.inspect(root, command, CancellationToken::new()).await).kind,
            RepositoryKind::Unborn
        );
    });
}

#[test]
fn keeps_other_roots_independent() {
    let (_directory, root, git, command) = fixture();
    let (_other_directory, other_root, _, _) = fixture();
    std::fs::write(other_root.join("other.txt"), "other checkout").unwrap();
    runtime().block_on(async {
        let (release, blocker) = hold();
        let mut first = Box::pin(git.inspect(root, command.clone(), CancellationToken::new()));
        pending(first.as_mut()).await;
        let (resume, separator) = hold();
        let mut second = Box::pin(git.inspect(other_root, command, CancellationToken::new()));
        pending(second.as_mut()).await;
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert!(status(first.await).entries.is_empty());
        pending(second.as_mut()).await;
        resume.send(()).unwrap();
        separator.await.unwrap();
        let result = status(second.await);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].path, "other.txt");
    });
}

#[test]
fn separates_reads_across_writes() {
    let (_directory, root, git, command) = fixture();
    runtime().block_on(async {
        let (release, blocker) = hold();
        let mut before =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(before.as_mut()).await;
        let (resume, separator) = hold();
        // The mutation worker clears join slots before and after physical changes.
        git.invalidate(&root);
        let mut after = Box::pin(git.inspect(root.clone(), command, CancellationToken::new()));
        pending(after.as_mut()).await;
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert!(status(before.await).entries.is_empty());
        std::fs::write(root.join("new.txt"), "after the original scan").unwrap();
        resume.send(()).unwrap();
        separator.await.unwrap();
        let result = status(after.await);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].path, "new.txt");
    });
}

#[tokio::test]
async fn releases_completed_and_failed_scans() {
    let (_directory, root, git, command) = fixture();
    let index = root.join(".git/index");
    std::fs::write(&index, "invalid index fixture").unwrap();
    let original = git.status(root.clone(), command.clone());
    let retained = original.clone();
    assert!(original.await.is_err());
    std::fs::remove_file(&index).unwrap();
    assert_eq!(
        status(
            git.inspect(root.clone(), command.clone(), CancellationToken::new())
                .await
        )
        .kind,
        RepositoryKind::Unborn
    );
    assert!(retained.await.is_err());
    let original = git.status(root.clone(), command.clone());
    let retained = original.clone();
    assert!(status(original.await).entries.is_empty());
    std::fs::write(root.join("fresh.txt"), "fresh scan").unwrap();
    let fresh = status(git.inspect(root, command, CancellationToken::new()).await);
    assert_eq!(fresh.entries.len(), 1);
    assert_eq!(fresh.entries[0].path, "fresh.txt");
    assert!(status(retained.await).entries.is_empty());
}
