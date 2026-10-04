use super::*;
use std::{future::Future, task::Poll};

#[test]
fn admits_parallel_reads() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    Repository::init(&root).unwrap();
    runtime.block_on(async {
        // Hold the blocking pool so every request is pending at the same time.
        let (release, held) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || held.recv().unwrap());
        let git = Git::new();
        let worktree = WorktreeId::new();
        let closed = CancellationToken::new();
        let mut reads = Box::pin(async {
            tokio::join!(
                git.inspect(
                    root.clone(),
                    Command::InspectGit { worktree },
                    closed.clone()
                ),
                git.inspect(
                    root.clone(),
                    Command::ListGitBranches { worktree },
                    closed.clone()
                ),
                git.inspect(
                    root.clone(),
                    Command::ReadGitLog {
                        worktree,
                        limit: 10,
                        cursor: None
                    },
                    closed.clone()
                ),
            )
        });
        std::future::poll_fn(|context| {
            assert!(reads.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        release.send(()).unwrap();
        blocker.await.unwrap();
        let (status, branches, history) = reads.await;
        assert!(matches!(status.unwrap(), Output::GitStatus(_)));
        assert!(matches!(branches.unwrap(), Output::GitBranches(_)));
        assert!(matches!(history.unwrap(), Output::GitLog(_)));
    });
}

#[tokio::test]
async fn shutdown_cancels_reads() {
    let closed = CancellationToken::new();
    closed.cancel();
    let error = Git::new()
        .inspect(
            PathBuf::new(),
            Command::InspectGit {
                worktree: WorktreeId::new(),
            },
            closed,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error.code,
        ErrorCode::Cancelled | ErrorCode::Unavailable
    ));
}
