use super::*;
use crate::store::{agent::checkpoints::restore::prepare, external::Work};

fn captured(
    fixture: &mut Fixture,
) -> (
    TurnId,
    Request,
    CheckpointId,
    WorktreeId,
    std::path::PathBuf,
) {
    let turn = running(fixture);
    let request = approved(fixture, turn, "file.txt", Some("before"), "after");
    capture(&mut fixture.database, turn, &request, Some("before")).unwrap();
    let file = list(
        &fixture.database.connection,
        fixture.session,
        turn,
        None,
        20,
    )
    .unwrap()
    .files
    .remove(0);
    let root = fixture.database.worktree_root(file.worktree).unwrap();
    (turn, request, file.id, file.worktree, root)
}

fn dispatch(fixture: &mut Fixture, request: Request) -> Work {
    let (reply, _) = oneshot::channel();
    fixture
        .database
        .dispatch(fixture.database.node, request, reply, &fixture.events)
        .unwrap()
}

fn rejected(fixture: &mut Fixture, command: Command) -> ErrorCode {
    let (reply, response) = oneshot::channel();
    let node = fixture.database.node;
    assert!(
        fixture
            .database
            .dispatch(node, Request::new(node, command), reply, &fixture.events)
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()
        .unwrap()
        .completion
        .blocking_recv()
        .unwrap()
        .unwrap_err()
        .code
}

fn finish(fixture: &mut Fixture, request: &Request, result: Result<Output, Fault>) {
    // Exercise each original receipt state without rerunning a physical mutation.
    fixture
        .database
        .connection
        .execute(
            "UPDATE requests SET status='admitted',result=NULL WHERE id=?1",
            [request.id.to_string()],
        )
        .unwrap();
    assert_eq!(
        fixture.database.finish_external(
            fixture.database.node,
            request,
            result.clone(),
            &fixture.events
        ),
        result
    );
}

#[test]
fn validates_status_outcome_and_scope() {
    let mut fixture = Fixture::new();
    let (turn, _request, id, worktree, root) = captured(&mut fixture);
    assert_eq!(
        prepare(&fixture.database, fixture.session, id, worktree, &root)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        prepare(&fixture.database, fixture.session, id, worktree, &root)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    // A scoped tool operation must reach admission while its turn is active.
    let mut fixture = Fixture::new();
    let (turn, request, id, worktree, root) = captured(&mut fixture);
    dispatch(&mut fixture, request.clone());
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        prepare(&fixture.database, fixture.session, id, worktree, &root)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    // An interrupted admitted write can be explicitly recovered, never implicitly replayed.
    fixture
        .database
        .connection
        .execute(
            "UPDATE requests SET status='unknown' WHERE id=?1",
            [request.id.to_string()],
        )
        .unwrap();
    assert_eq!(
        prepare(&fixture.database, fixture.session, id, worktree, &root)
            .unwrap()
            .file
            .outcome,
        RequestOutcome::Unknown
    );
    for code in [ErrorCode::RevisionConflict, ErrorCode::OutcomeUnknown] {
        finish(
            &mut fixture,
            &request,
            Err(Fault::new(code, "injected write outcome")),
        );
        let result = prepare(&fixture.database, fixture.session, id, worktree, &root);
        if code == ErrorCode::OutcomeUnknown {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::Conflict);
        }
    }
    let written = FileWritten {
        path: "file.txt".into(),
        revision: version("after").revision,
        size: 5,
    };
    finish(
        &mut fixture,
        &request,
        Ok(Output::FileWritten(written.clone())),
    );
    prepare(&fixture.database, fixture.session, id, worktree, &root).unwrap();
    assert_eq!(
        prepare(&fixture.database, SessionId::new(), id, worktree, &root)
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        prepare(
            &fixture.database,
            fixture.session,
            id,
            WorktreeId::new(),
            &root
        )
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(
        prepare(
            &fixture.database,
            fixture.session,
            id,
            worktree,
            &root.join("changed")
        )
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    finish(
        &mut fixture,
        &request,
        Ok(Output::FileWritten(FileWritten {
            path: "another.txt".into(),
            ..written
        })),
    );
    assert_eq!(
        prepare(&fixture.database, fixture.session, id, worktree, &root)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn rechecks_queued_membership() {
    let mut fixture = Fixture::new();
    let (turn, write, id, worktree, root) = captured(&mut fixture);
    dispatch(&mut fixture, write.clone());
    finish(
        &mut fixture,
        &write,
        Ok(Output::FileWritten(FileWritten {
            path: "file.txt".into(),
            revision: version("after").revision,
            size: 5,
        })),
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    std::fs::write(root.join("file.txt"), "after").unwrap();
    let request = Request::new(
        fixture.database.node,
        Command::RestoreFileCheckpoint {
            session: fixture.session,
            checkpoint: id,
            worktree,
        },
    );
    assert!(request.command.durable());
    let Work::Mutation(pending) = dispatch(&mut fixture, request.clone()) else {
        panic!("mutation expected")
    };
    assert_eq!(pending.roots.target, root);
    let registration = Command::RegisterProject {
        name: "Queued target".into(),
        path: root.join("file.txt").to_str().unwrap().into(),
    };
    assert_eq!(
        rejected(&mut fixture, registration.clone()),
        ErrorCode::Busy
    );
    let session = fixture.session;
    let Output::Rewound(rewind) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session,
            through: None,
            expected_head: turn,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected")
    };
    let result = prepare(&fixture.database, fixture.session, id, worktree, &root).unwrap_err();
    assert_eq!(result.code, ErrorCode::WrongTarget);
    finish(&mut fixture, &request, Err(result));
    assert_eq!(
        rejected(&mut fixture, registration),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file.txt")).unwrap(),
        "after"
    );
    prepare(&fixture.database, rewind.backup.id, id, worktree, &root).unwrap();
}

#[test]
fn rechecks_after_rewind() {
    let mut fixture = Fixture::new();
    let (turn, write, id, worktree, root) = captured(&mut fixture);
    dispatch(&mut fixture, write.clone());
    finish(
        &mut fixture,
        &write,
        Ok(Output::FileWritten(FileWritten {
            path: "file.txt".into(),
            revision: version("after").revision,
            size: 5,
        })),
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    std::fs::write(root.join("file.txt"), "after").unwrap();
    let path = std::path::PathBuf::from(fixture.database.connection.path().unwrap());
    let node = fixture.database.node;
    fixture.database.close().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        use sailry_client::Client;
        use sailry_link::Local;
        let (entered, mut waiting) = tokio::sync::mpsc::unbounded_channel();
        let (release, released) = std::sync::mpsc::channel();
        let store = crate::store::Store::start(
            path,
            node,
            None,
            Default::default(),
            Default::default(),
            Default::default(),
            move |roots, request| {
                if matches!(&request.command, Command::WriteFile { path, .. } if path == "blocked")
                {
                    entered.send(()).unwrap();
                    released
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                }
                crate::store::mutations::execute(roots, request)
            },
        )
        .await
        .unwrap();
        let client = Client::new(Arc::new(Local::new(node, node, store.ingress.clone())));
        let gate = client
            .dispatch(client.prepare(Command::WriteFile {
                worktree,
                path: "blocked".into(),
                text: "gate".into(),
                expected_revision: None,
            }))
            .await
            .unwrap();
        waiting.recv().await.unwrap();
        let request = client.prepare(Command::RestoreFileCheckpoint {
            session: fixture.session,
            checkpoint: id,
            worktree,
        });
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap();
        let Output::Rewound(rewind) = client
            .execute(client.prepare(Command::RewindConversation {
                session: fixture.session,
                through: None,
                expected_head: turn,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        release.send(()).unwrap();
        gate.completion.await.unwrap().unwrap();
        assert_eq!(
            pending.completion.await.unwrap().unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            std::fs::read_to_string(root.join("file.txt")).unwrap(),
            "after"
        );
        client
            .execute(client.prepare(Command::RestoreFileCheckpoint {
                session: rewind.backup.id,
                checkpoint: id,
                worktree,
            }))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file.txt")).unwrap(),
            "before"
        );
        store.shutdown().await.unwrap();
    });
}
