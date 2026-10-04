use super::*;
use crate::trash_fixture::Entry;
use std::{fs, time::Duration};

#[tokio::test]
#[ignore = "moves uniquely named cross-Node fixtures into native Trash and recovers them"]
async fn recovers_offline_completion() {
    let fixture = Fixture::start().await;
    let mut completed = Vec::new();
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        for empty in [false, true] {
            let entry = Entry::new(
                &fixture.source.root,
                format!("sailry-node-move-{}-文件", RequestId::new()),
            );
            let bytes = if empty {
                vec![]
            } else {
                vec![0xff; 10 * 1024 * 1024]
            };
            fs::write(&entry.source, &bytes).unwrap();
            let mut source_watch = fixture.source.transports[pair.0]
                .subscribe(Topic::Files(fixture.source.worktree))
                .await
                .unwrap();
            let mut target_watch = fixture.destination.transports[pair.1]
                .subscribe(Topic::Files(fixture.destination.worktree))
                .await
                .unwrap();
            source_watch.next().await.unwrap();
            target_watch.next().await.unwrap();
            let before = (fixture.source.count(), fixture.destination.count());
            let download = prepare(&source, fixture.source.worktree, &entry.name).await;
            let pending = target
                .stage_move(
                    &source,
                    &download,
                    spec(&download, fixture.destination.worktree, &entry.name),
                    CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(
                (fixture.source.count(), fixture.destination.count()),
                before
            );
            let written = target
                .finish_move(&source, &pending, |_| {
                    assert!(entry.source.exists());
                    assert_eq!(
                        fs::read(fixture.destination.root.join(&entry.name)).unwrap(),
                        bytes
                    );
                })
                .await
                .unwrap();
            assert!(!entry.source.exists());
            assert_eq!(fs::read(entry.recover()).unwrap(), bytes);
            assert_eq!(
                fs::read(fixture.destination.root.join(&entry.name)).unwrap(),
                bytes
            );
            assert_eq!(
                (fixture.source.count(), fixture.destination.count()),
                (before.0 + 1, before.1 + 1)
            );
            for watch in [&mut source_watch, &mut target_watch] {
                assert!(matches!(
                    tokio::time::timeout(Duration::from_secs(5), watch.next())
                        .await
                        .unwrap()
                        .unwrap(),
                    Update::FilesChanged { .. }
                ));
            }
            fs::write(&entry.source, "source replacement").unwrap();
            fs::write(
                fixture.destination.root.join(&entry.name),
                "destination edit",
            )
            .unwrap();
            assert_eq!(
                target.finish_move(&source, &pending, |_| {}).await.unwrap(),
                written
            );
            assert!(!entry.recycled.exists());
            completed.push((pair, pending, written));
        }
    }
    // Recover the original source result without even contacting the destination.
    fixture.destination.node.shutdown().await.unwrap();
    fixture.source.node.shutdown().await.unwrap();
    let node = Node::start(&fixture.source.profile).await.unwrap();
    let sources = [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ];
    for (pair, pending, written) in completed {
        let source = Client::new(sources[pair.0].clone());
        let target = Client::new(fixture.destination.transports[pair.1].clone());
        assert_eq!(
            target.finish_move(&source, &pending, |_| {}).await.unwrap(),
            written
        );
        assert_eq!(
            fs::read(fixture.source.root.join(&written.path)).unwrap(),
            b"source replacement"
        );
        assert_eq!(
            fs::read(fixture.destination.root.join(&written.path)).unwrap(),
            b"destination edit"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "moves uniquely named cross-Node fixtures into native Trash and recovers them"]
async fn recovery_preserves_replacements() {
    let fixture = Fixture::start().await;
    let db =
        rusqlite::Connection::open(fixture.source.profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut pending_moves = Vec::new();
    for pair in PATHS {
        let entry = Entry::new(
            &fixture.source.root,
            format!("sailry-node-move-{}-文件", RequestId::new()),
        );
        fs::write(&entry.source, "original").unwrap();
        let (source, target) = fixture.clients(pair);
        let download = prepare(&source, fixture.source.worktree, &entry.name).await;
        let pending = target
            .stage_move(
                &source,
                &download,
                spec(&download, fixture.destination.worktree, &entry.name),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let mut confirmed = false;
        assert_eq!(
            target
                .finish_move(&source, &pending, |_| confirmed = true)
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert!(confirmed);
        assert!(!entry.source.exists());
        assert_eq!(fs::read(entry.recover()).unwrap(), b"original");
        fs::write(&entry.source, "replacement").unwrap();
        fs::write(fixture.destination.root.join(&entry.name), "external edit").unwrap();
        assert_eq!(
            target
                .finish_move(&source, &pending, |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!entry.recycled.exists());
        pending_moves.push((pair, pending, entry.name.clone()));
    }
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    fixture.destination.node.shutdown().await.unwrap();
    fixture.source.node.shutdown().await.unwrap();
    let node = Node::start(&fixture.source.profile).await.unwrap();
    let sources = [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ];
    for (pair, pending, path) in pending_moves {
        let source = Client::new(sources[pair.0].clone());
        let target = Client::new(fixture.destination.transports[pair.1].clone());
        assert_eq!(
            target
                .finish_move(&source, &pending, |_| {})
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            fs::read(fixture.source.root.join(&path)).unwrap(),
            b"replacement"
        );
        assert_eq!(
            fs::read(fixture.destination.root.join(path)).unwrap(),
            b"external edit"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
