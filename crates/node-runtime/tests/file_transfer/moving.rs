use super::*;

#[tokio::test]
async fn staging_requires_source_identity() {
    let fixture = Fixture::start().await;
    std::fs::write(fixture.source.root.join("file"), "source").unwrap();
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        let before = (fixture.source.count(), fixture.destination.count());
        let mut download = prepare(&source, fixture.source.worktree, "file").await;
        download.stamp.clear();
        assert_eq!(
            target
                .stage_move(
                    &source,
                    &download,
                    spec(&download, fixture.destination.worktree, "copy"),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(source.open(download.stream).await.is_err());
        let download = prepare(&source, fixture.source.worktree, "file").await;
        assert_eq!(
            source
                .stage_move(
                    &source,
                    &download,
                    spec(&download, fixture.source.worktree, "copy"),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(source.open(download.stream).await.is_err());
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let mut replacement = spec(&download, fixture.destination.worktree, "copy");
        replacement.expected_revision = Some(download.revision.clone());
        assert_eq!(
            target
                .stage_move(
                    &source,
                    &download,
                    replacement,
                    CancellationToken::new(),
                    |_| {}
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(source.open(download.stream).await.is_err());
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            target
                .stage_move(
                    &source,
                    &download,
                    spec(&download, fixture.destination.worktree, "copy"),
                    cancelled,
                    |_| {},
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(source.open(download.stream).await.is_err());
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let pending = target
            .stage_move(
                &source,
                &download,
                spec(&download, fixture.destination.worktree, "copy"),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert!(matches!(
            pending.recycle().command,
            Command::TrashFile { .. }
        ));
        assert_eq!(
            source.outcome(pending.recycle()).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert_eq!(
            target.outcome(pending.publication()).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        assert_eq!(
            source
                .finish_move(&target, &pending, |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            (fixture.source.count(), fixture.destination.count()),
            before
        );
        assert_eq!(
            std::fs::read(fixture.source.root.join("file")).unwrap(),
            b"source"
        );
        assert!(!fixture.destination.root.join("copy").exists());
        let Command::FinishFileUpload { stream, .. } = pending.publication().command else {
            panic!("publication expected")
        };
        target
            .execute(target.prepare(Command::CancelFileTransfer { stream }))
            .await
            .unwrap();
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_changed_copies() {
    let fixture = Fixture::start().await;
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        for changed_source in [false, true] {
            let path = format!("{}-{}-{changed_source}", pair.0, pair.1);
            std::fs::write(fixture.source.root.join(&path), "original").unwrap();
            let download = prepare(&source, fixture.source.worktree, &path).await;
            let pending = target
                .stage_move(
                    &source,
                    &download,
                    spec(&download, fixture.destination.worktree, &path),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            let written = target.execute(pending.publication().clone()).await.unwrap();
            let modified = if changed_source {
                &fixture.source.root
            } else {
                &fixture.destination.root
            };
            if changed_source {
                // Same contents in a newly allocated file must still fail the source guard.
                std::fs::rename(
                    modified.join(&path),
                    modified.join(format!("retained-{path}")),
                )
                .unwrap();
                std::fs::write(modified.join(&path), "original").unwrap();
            } else {
                std::fs::write(modified.join(&path), "external").unwrap();
            }
            let mut confirmed = None;
            assert_eq!(
                target
                    .finish_move(&source, &pending, |written| confirmed =
                        Some(written.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            assert_eq!(confirmed.map(Output::FileWritten), Some(written));
            let outcome = source.outcome(pending.recycle()).await.unwrap();
            if changed_source {
                assert!(
                    matches!(outcome, RequestOutcome::Completed(result) if result.as_ref().as_ref().unwrap_err().code == ErrorCode::RevisionConflict)
                );
            } else {
                assert_eq!(outcome, RequestOutcome::NotAdmitted);
            }
            assert_eq!(
                std::fs::read(fixture.source.root.join(&path)).unwrap(),
                b"original"
            );
            assert_eq!(
                std::fs::read(fixture.destination.root.join(&path)).unwrap(),
                if changed_source {
                    b"original"
                } else {
                    b"external"
                }
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_uncertain_source() {
    let fixture = Fixture::start().await;
    let db = rusqlite::Connection::open(fixture.destination.profile.join("storage/node.sqlite3"))
        .unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        let path = format!("{}-{}", pair.0, pair.1);
        std::fs::write(fixture.source.root.join(&path), "original").unwrap();
        let download = prepare(&source, fixture.source.worktree, &path).await;
        let pending = target
            .stage_move(
                &source,
                &download,
                spec(&download, fixture.destination.worktree, &path),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(
            target
                .finish_move(&source, &pending, |_| panic!(
                    "uncertain publication was confirmed"
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            source.outcome(pending.recycle()).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        std::fs::write(fixture.destination.root.join(&path), "external").unwrap();
        assert_eq!(
            target
                .finish_move(&source, &pending, |_| panic!(
                    "uncertain publication was confirmed"
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(fixture.source.root.join(&path)).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(fixture.destination.root.join(path)).unwrap(),
            b"external"
        );
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    fixture.close().await;
}

#[cfg(target_os = "macos")]
#[path = "moving/native.rs"]
mod native;
