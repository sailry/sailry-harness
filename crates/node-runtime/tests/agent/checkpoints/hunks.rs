use super::*;
use fixture::Fixture;
use sailry_protocol::conversation::checkpoint::hunk;

fn source() -> String {
    (0..30).map(|line| format!("line {line}\n")).collect()
}

async fn review(fixture: &Fixture) -> hunk::Review {
    let Output::TurnHunks(review) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadTurnHunks {
            session: fixture.session.id,
            turn: fixture.turn,
            path: "source.txt".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("hunks expected")
    };
    review
}

async fn status(fixture: &Fixture) -> GitStatus {
    let Output::GitStatus(status) = fixture
        .client
        .execute(fixture.client.prepare(Command::InspectGit {
            worktree: fixture.session.worktree,
        }))
        .await
        .unwrap()
    else {
        panic!("Git status expected")
    };
    status
}

fn selection(
    fixture: &Fixture,
    hunk: &hunk::Hunk,
    current: &str,
    operation: hunk::Operation,
    status: &GitStatus,
) -> hunk::Selection {
    let stage = operation == hunk::Operation::Stage;
    hunk::Selection {
        session: fixture.session.id,
        turn: fixture.turn,
        worktree: fixture.session.worktree,
        path: "source.txt".into(),
        hunk: hunk.id.clone(),
        operation,
        expected_revision: blake3::hash(current.as_bytes()).to_hex().to_string(),
        expected_index: if stage {
            status.index_revision.clone()
        } else {
            None
        },
        expected_head: if stage { status.head.clone() } else { None },
    }
}

fn indexed(fixture: &Fixture, path: &str) -> String {
    let repository = git2::Repository::open(&fixture.root).unwrap();
    let index = repository.index().unwrap();
    let entry = index.get_path(std::path::Path::new(path), 0).unwrap();
    String::from_utf8(repository.find_blob(entry.id).unwrap().content().to_vec()).unwrap()
}

#[tokio::test]
#[cfg(unix)]
async fn stages_and_rejects_selected_blocks() {
    for remote in [false, true] {
        let before = source();
        let after = before
            .replace("line 2\n", "changed 2\n")
            .replace("line 26\n", "changed 26\n");
        let fixture = Fixture::git(remote, &[("source.txt", Some(&before), &after)]).await;
        let original = review(&fixture).await;
        assert_eq!(original.hunks.len(), 2);
        assert!(!original.partial);
        let counts = fixture.counts().await;
        let current = after.replace("line 15\n", "manual 15\n");
        std::fs::write(fixture.root.join("source.txt"), &current).unwrap();
        std::fs::write(fixture.root.join("manual.txt"), "Unrelated staged data\n").unwrap();
        let snapshot = status(&fixture).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::UpdateGitIndex {
                worktree: fixture.session.worktree,
                paths: vec!["manual.txt".into()],
                operation: GitIndexChange::Stage,
                expected_index: snapshot.index_revision.unwrap(),
                expected_head: snapshot.head,
            }))
            .await
            .unwrap();
        let snapshot = status(&fixture).await;
        let request = fixture.client.prepare(Command::ApplyTurnHunk(selection(
            &fixture,
            &original.hunks[0],
            &current,
            hunk::Operation::Stage,
            &snapshot,
        )));
        let first = fixture.client.execute(request.clone()).await.unwrap();
        assert!(matches!(&first, Output::HunkApplied(value) if value.index.is_some()));
        assert_eq!(
            indexed(&fixture, "source.txt"),
            before.replace("line 2\n", "changed 2\n")
        );
        assert_eq!(indexed(&fixture, "manual.txt"), "Unrelated staged data\n");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            current
        );
        let bytes = std::fs::read(fixture.root.join(".git/index")).unwrap();
        assert_eq!(fixture.client.execute(request).await.unwrap(), first);
        assert_eq!(
            std::fs::read(fixture.root.join(".git/index")).unwrap(),
            bytes
        );
        let rejection = fixture.client.prepare(Command::ApplyTurnHunk(selection(
            &fixture,
            &original.hunks[1],
            &current,
            hunk::Operation::Reject,
            &snapshot,
        )));
        let output = fixture.client.execute(rejection.clone()).await.unwrap();
        let expected = current.replace("changed 26\n", "line 26\n");
        assert!(
            matches!(&output, Output::HunkApplied(value) if value.index.is_none() && value.revision == blake3::hash(expected.as_bytes()).to_hex().as_str())
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read(fixture.root.join(".git/index")).unwrap(),
            bytes
        );
        assert_eq!(fixture.client.execute(rejection).await.unwrap(), output);
        assert_eq!(review(&fixture).await, original);
        assert_eq!(fixture.counts().await, counts);
        assert_eq!(
            fixture
                .client
                .read_conversation(fixture.session.id, None, 100)
                .await
                .unwrap()
                .page,
            fixture.history
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_stale_and_forged_selections() {
    for remote in [false, true] {
        let before = source();
        let after = before.replace("line 2\n", "changed 2\n");
        let fixture = Fixture::git(remote, &[("source.txt", Some(&before), &after)]).await;
        let original = review(&fixture).await;
        let snapshot = status(&fixture).await;
        let base = selection(
            &fixture,
            &original.hunks[0],
            &after,
            hunk::Operation::Reject,
            &snapshot,
        );
        for (selected, code) in [
            (
                hunk::Selection {
                    session: SessionId::new(),
                    ..base.clone()
                },
                ErrorCode::WrongTarget,
            ),
            (
                hunk::Selection {
                    worktree: WorktreeId::new(),
                    ..base.clone()
                },
                ErrorCode::NotFound,
            ),
            (
                hunk::Selection {
                    hunk: "0".repeat(64),
                    ..base.clone()
                },
                ErrorCode::NotFound,
            ),
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ApplyTurnHunk(selected)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
                after
            );
        }
        std::fs::write(
            fixture.root.join("source.txt"),
            after.replace("line 1\n", "manual 1\n"),
        )
        .unwrap();
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ApplyTurnHunk(base.clone())))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let duplicate = format!("{after}{after}");
        std::fs::write(fixture.root.join("source.txt"), &duplicate).unwrap();
        let selected = hunk::Selection {
            expected_revision: blake3::hash(duplicate.as_bytes()).to_hex().to_string(),
            ..base
        };
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ApplyTurnHunk(selected)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            duplicate
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
#[cfg(unix)]
async fn preserves_index_when_guards_fail() {
    for remote in [false, true] {
        let before = source();
        let after = before.replace("line 2\n", "changed 2\n");
        let fixture = Fixture::git(remote, &[("source.txt", Some(&before), &after)]).await;
        let original = review(&fixture).await;
        let snapshot = status(&fixture).await;
        let selected = selection(
            &fixture,
            &original.hunks[0],
            &after,
            hunk::Operation::Stage,
            &snapshot,
        );
        let bytes = std::fs::read(fixture.root.join(".git/index")).unwrap();
        for selected in [
            hunk::Selection {
                expected_index: Some("0".repeat(64)),
                ..selected.clone()
            },
            hunk::Selection {
                expected_head: Some("0".repeat(40)),
                ..selected.clone()
            },
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ApplyTurnHunk(selected)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::RevisionConflict
            );
            assert_eq!(
                std::fs::read(fixture.root.join(".git/index")).unwrap(),
                bytes
            );
        }
        std::fs::write(fixture.root.join(".git/index.lock"), "Another writer").unwrap();
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::ApplyTurnHunk(selected.clone()))
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join(".git/index.lock")).unwrap(),
            "Another writer"
        );
        std::fs::remove_file(fixture.root.join(".git/index.lock")).unwrap();
        let outside = fixture.directory.path().join("outside.txt");
        std::fs::write(&outside, &after).unwrap();
        std::fs::remove_file(fixture.root.join("source.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("source.txt")).unwrap();
        for operation in [hunk::Operation::Stage, hunk::Operation::Reject] {
            let selected = hunk::Selection {
                operation,
                expected_index: if operation == hunk::Operation::Stage {
                    snapshot.index_revision.clone()
                } else {
                    None
                },
                expected_head: if operation == hunk::Operation::Stage {
                    snapshot.head.clone()
                } else {
                    None
                },
                ..selected.clone()
            };
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ApplyTurnHunk(selected)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), after);
            assert_eq!(
                std::fs::read(fixture.root.join(".git/index")).unwrap(),
                bytes
            );
        }
        assert!(!fixture.root.join(".git/index.lock").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
#[cfg(unix)]
async fn recovers_original_receipts_across_restart() {
    for remote in [false, true] {
        let before = source();
        let after = before
            .replace("line 2\n", "changed 2\n")
            .replace("line 26\n", "changed 26\n");
        let fixture = Fixture::git(remote, &[("source.txt", Some(&before), &after)]).await;
        let original = review(&fixture).await;
        let snapshot = status(&fixture).await;
        let counts = fixture.counts().await;
        let staged = fixture.client.prepare(Command::ApplyTurnHunk(selection(
            &fixture,
            &original.hunks[0],
            &after,
            hunk::Operation::Stage,
            &snapshot,
        )));
        let admission = fixture.client.dispatch(staged.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission.completion);
        let output = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) =
                    fixture.client.outcome(&staged).await.unwrap()
                {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let bytes = std::fs::read(fixture.root.join(".git/index")).unwrap();
        assert_eq!(
            indexed(&fixture, "source.txt"),
            before.replace("line 2\n", "changed 2\n")
        );
        let rejected = fixture.client.prepare(Command::ApplyTurnHunk(selection(
            &fixture,
            &original.hunks[1],
            &after,
            hunk::Operation::Reject,
            &snapshot,
        )));
        fixture.node.with_storage(|db| Box::pin(async move {
            db.execute_batch("CREATE TRIGGER lose_hunk BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;")
                .await.map_err(crate::database_fixture::fault)
        })).await.unwrap();
        assert_eq!(
            fixture
                .client
                .execute(rejected.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            after.replace("changed 26\n", "line 26\n")
        );
        fixture
            .node
            .with_storage(|db| {
                Box::pin(async move {
                    db.execute_batch("DROP TRIGGER lose_hunk")
                        .await
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        let replacement = "Manual replacement survives original-ID replay\n";
        std::fs::write(fixture.root.join("source.txt"), replacement).unwrap();
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(staged.clone()).await.unwrap(), output);
        assert_eq!(
            client.execute(rejected.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            client.outcome(&rejected).await.unwrap(),
            RequestOutcome::Unknown
        );
        assert_eq!(
            std::fs::read(fixture.root.join(".git/index")).unwrap(),
            bytes
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            replacement
        );
        assert_eq!(history(&client, fixture.session.id).await, fixture.history);
        let actual: (i64, i64) = node.with_storage(|database| Box::pin(async move {
            let row = database
                .prepare("SELECT (SELECT count(*) FROM agent_events),(SELECT count(*) FROM file_checkpoints)")
                .await.map_err(crate::database_fixture::fault)?
                .query_row(()).await.map_err(crate::database_fixture::fault)?;
            Ok((row.get(0).map_err(crate::database_fixture::fault)?, row.get(1).map_err(crate::database_fixture::fault)?))
        })).await.unwrap();
        assert_eq!(actual, counts);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn denies_plugin_access_to_native_review() {
    for remote in [false, true] {
        let before = source();
        let after = before.replace("line 2\n", "changed 2\n");
        let fixture = Fixture::git(remote, &[("source.txt", Some(&before), &after)]).await;
        let original = review(&fixture).await;
        let snapshot = status(&fixture).await;
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "files".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        let context = sailry_protocol::plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        };
        for command in [
            Command::ReadTurnHunks {
                session: fixture.session.id,
                turn: fixture.turn,
                path: "source.txt".into(),
            },
            Command::ApplyTurnHunk(selection(
                &fixture,
                &original.hunks[0],
                &after,
                hunk::Operation::Reject,
                &snapshot,
            )),
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            after
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
