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

#[tokio::test]
async fn reports_bounded_line_counts() {
    let (_directory, root, git, command) = fixture();
    std::fs::write(root.join("small.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("large.txt"), "large line\n".repeat(110_000)).unwrap();
    let result = status(git.inspect(root, command, CancellationToken::new()).await);
    let small = result
        .entries
        .iter()
        .find(|entry| entry.path == "small.txt")
        .unwrap();
    assert_eq!(small.diff.additions, 2);
    assert!(!small.diff.truncated);
    let large = result
        .entries
        .iter()
        .find(|entry| entry.path == "large.txt")
        .unwrap();
    assert!(large.diff.truncated);
    assert!(large.unstaged_diff.truncated);
    assert!(!large.staged_diff.truncated);
    assert!(!result.truncated, "path inventory remains complete");
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
fn queued_reads_keep_execution_budget() {
    let (_directory, root, git, command) = fixture();
    runtime().block_on(async {
        tokio::time::pause();
        let gate = git.scans.root(&root);
        let permit = gate.acquire_owned().await.unwrap();
        let (release, blocker) = hold();
        let mut read = Box::pin(git.inspect(root, command, CancellationToken::new()));
        pending(read.as_mut()).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        drop(permit);
        pending(read.as_mut()).await;
        tokio::time::advance(Duration::from_secs(2)).await;
        pending(read.as_mut()).await;
        tokio::time::resume();
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert_eq!(status(read.await).kind, RepositoryKind::Unborn);
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
        git.invalidate(&root);
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
        let mut after =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(after.as_mut()).await;
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert!(status(before.await).entries.is_empty());
        let mut joined = Box::pin(git.inspect(root.clone(), command, CancellationToken::new()));
        pending(joined.as_mut()).await;
        std::fs::write(root.join("new.txt"), "after the original scan").unwrap();
        resume.send(()).unwrap();
        separator.await.unwrap();
        let result = status(after.await);
        assert_eq!(status(joined.await), result);
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
    let cached = status(
        git.inspect(root.clone(), command.clone(), CancellationToken::new())
            .await,
    );
    assert!(
        cached.entries.is_empty(),
        "successful scans now remain cached"
    );
    git.invalidate(&root);
    let fresh = status(git.inspect(root, command, CancellationToken::new()).await);
    assert_eq!(fresh.entries.len(), 1);
    assert_eq!(fresh.entries[0].path, "fresh.txt");
    assert!(status(retained.await).entries.is_empty());
}

#[tokio::test]
async fn expires_without_extending_hits() {
    let (_directory, root, git, command) = fixture();
    let before = tokio::time::Instant::now();
    let original = status(
        git.inspect(root.clone(), command.clone(), CancellationToken::new())
            .await,
    );
    let expires = {
        let slots = git.statuses.lock().unwrap();
        let Some(StatusSlot::Complete(cached)) = slots.get(&root) else {
            panic!("successful scan should be cached")
        };
        cached.expires
    };
    assert!(expires >= before + STATUS_TTL);
    std::fs::write(root.join("fresh.txt"), "after the cached scan").unwrap();
    assert_eq!(
        status(
            git.clone()
                .inspect(root.clone(), command.clone(), CancellationToken::new())
                .await
        ),
        original
    );
    {
        let mut slots = git.statuses.lock().unwrap();
        let Some(StatusSlot::Complete(cached)) = slots.get_mut(&root) else {
            panic!("cache hit should retain the successful scan")
        };
        assert_eq!(cached.expires, expires, "hits must not slide the TTL");
        // Set the existing clock boundary directly instead of sleeping in the fixture.
        cached.expires = tokio::time::Instant::now();
    }
    let index = root.join(".git/index");
    std::fs::write(&index, "invalid index fixture").unwrap();
    assert!(
        git.inspect(root.clone(), command.clone(), CancellationToken::new())
            .await
            .is_err()
    );
    assert!(!git.statuses.lock().unwrap().contains_key(&root));
    std::fs::remove_file(index).unwrap();
    let fresh = status(git.inspect(root, command, CancellationToken::new()).await);
    assert_eq!(fresh.entries.len(), 1);
    assert_eq!(fresh.entries[0].path, "fresh.txt");
}

#[tokio::test]
async fn invalidates_only_the_selected_root() {
    let (_directory, root, git, command) = fixture();
    let (_other_directory, other, _, _) = fixture();
    for root in [&root, &other] {
        assert!(
            status(
                git.inspect(root.clone(), command.clone(), CancellationToken::new())
                    .await
            )
            .entries
            .is_empty()
        );
        std::fs::write(root.join("new.txt"), "changed after inspection").unwrap();
    }
    git.invalidate(&root);
    let changed = status(
        git.inspect(root, command.clone(), CancellationToken::new())
            .await,
    );
    assert_eq!(changed.entries.len(), 1);
    assert!(
        status(git.inspect(other, command, CancellationToken::new()).await)
            .entries
            .is_empty()
    );
}

#[test]
fn invalidation_prevents_late_publication() {
    let (_directory, root, git, command) = fixture();
    runtime().block_on(async {
        let (release, blocker) = hold();
        let mut stale =
            Box::pin(git.inspect(root.clone(), command.clone(), CancellationToken::new()));
        pending(stale.as_mut()).await;
        git.invalidate(&root);
        release.send(()).unwrap();
        blocker.await.unwrap();
        assert!(status(stale.await).entries.is_empty());
        assert!(!git.statuses.lock().unwrap().contains_key(&root));
        std::fs::write(root.join("new.txt"), "after the invalidated scan").unwrap();
        let fresh = status(git.inspect(root, command, CancellationToken::new()).await);
        assert_eq!(fresh.entries.len(), 1);
    });
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_replaced_cached_roots() {
    let (_directory, root, git, command) = fixture();
    assert!(
        status(
            git.inspect(root.clone(), command.clone(), CancellationToken::new())
                .await
        )
        .entries
        .is_empty()
    );
    let moved = tempfile::tempdir().unwrap();
    let retained = moved.path().join("original");
    std::fs::rename(&root, &retained).unwrap();
    Repository::init(&root).unwrap();
    std::fs::write(root.join("replacement.txt"), "different checkout").unwrap();
    assert_eq!(
        git.inspect(root.clone(), command.clone(), CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let fresh = status(git.inspect(root, command, CancellationToken::new()).await);
    assert_eq!(fresh.entries.len(), 1);
    assert_eq!(fresh.entries[0].path, "replacement.txt");
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_cached_metadata_symlinks() {
    use std::os::unix::fs::symlink;
    let (_directory, root, git, command) = fixture();
    status(
        git.inspect(root.clone(), command.clone(), CancellationToken::new())
            .await,
    );
    let outside = tempfile::tempdir().unwrap();
    let metadata = outside.path().join("metadata");
    std::fs::rename(root.join(".git"), &metadata).unwrap();
    symlink(&metadata, root.join(".git")).unwrap();
    let error = git
        .inspect(root.clone(), command.clone(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    std::fs::remove_file(root.join(".git")).unwrap();
    std::fs::rename(metadata, root.join(".git")).unwrap();
    assert_eq!(
        status(git.inspect(root, command, CancellationToken::new()).await).kind,
        RepositoryKind::Unborn
    );
}

#[cfg(unix)]
#[test]
fn reuses_confined_parents_and_missing_paths() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("nested/deep")).unwrap();
    std::fs::write(root.join("nested/deep/first.txt"), "first").unwrap();
    std::fs::write(root.join("nested/deep/second.txt"), "second").unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.join("escape")).unwrap();
    let mut parents = EntryParents::new(path::root(&root).unwrap());
    parents.check("nested/deep/first.txt", false).unwrap();
    let retained = path::identity::directory(parents.stack[2].1.as_ref().unwrap()).unwrap();
    parents.check("nested/deep/second.txt", false).unwrap();
    assert_eq!(parents.stack.len(), 3);
    assert_eq!(
        path::identity::directory(parents.stack[2].1.as_ref().unwrap()).unwrap(),
        retained
    );
    parents.check("missing/deep/first.txt", false).unwrap();
    parents.check("missing/deep/second.txt", false).unwrap();
    assert!(parents.stack[1].1.is_none());
    assert!(parents.check("escape/private.txt", false).is_err());
    assert!(parents.check(".sailry/worktrees/id/file", false).is_err());
    assert!(parents.check("../outside", false).is_err());
}
