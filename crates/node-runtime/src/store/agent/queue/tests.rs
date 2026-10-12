use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

#[test]
fn pauses_unclaimed_input_after_restart() {
    for ready in [false, true] {
        let mut fixture = Fixture::new();
        let turn = fixture.queued();
        if ready {
            runs::start(&fixture.database.connection, turn).unwrap();
        }
        let message = message(&fixture.database.connection, turn).unwrap();
        let before = read(&fixture.database.connection, fixture.session).unwrap();
        assert!(!before.paused);
        assert_eq!(before.items[0].ready, ready);
        let path = fixture
            ._root
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let node = fixture.database.node;
        fixture.database.close().unwrap();
        let mut database = Database::open(&path, node, None).unwrap();
        let recovered = read(&database.connection, fixture.session).unwrap();
        assert!(recovered.paused);
        assert_eq!(recovered.revision, before.revision + 1);
        assert_eq!(recovered.items, before.items);
        assert_eq!(super::message(&database.connection, turn).unwrap(), message);
        assert_eq!(
            runs::get(&database.connection, turn).unwrap().status,
            Status::Queued
        );
        assert!(
            runs::claim(&mut database, &fixture.events)
                .unwrap()
                .is_none()
        );

        database.close().unwrap();
        let mut database = Database::open(&path, node, None).unwrap();
        assert_eq!(
            read(&database.connection, fixture.session).unwrap(),
            recovered
        );
        runs::start(&database.connection, turn).unwrap();
        assert!(
            runs::claim(&mut database, &fixture.events)
                .unwrap()
                .is_none()
        );
        let Output::QueuedTurn(fresh) = command(
            &mut database,
            &fixture.events,
            Command::SubmitTurn {
                session: fixture.session,
                expected_revision: message.turn.revision,
                message: "Fresh input".into(),
            },
        ) else {
            panic!("queued turn expected")
        };
        let invocation = runs::claim(&mut database, &fixture.events)
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, fresh);
        assert_eq!(invocation.message.text, "Fresh input");
        runs::finish(
            &mut database,
            fresh.id,
            Status::Completed,
            None,
            &fixture.events,
        )
        .unwrap();
        let invocation = runs::claim(&mut database, &fixture.events)
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, message.turn);
        assert_eq!(invocation.message, message.message);
        database.close().unwrap();
    }
}

#[test]
fn fresh_input_precedes_stopped_queue() {
    let mut fixture = Fixture::new();
    let Output::QueuedTurn(active) = command(
        &mut fixture.database,
        &fixture.events,
        Command::SubmitTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Active input".into(),
        },
    ) else {
        panic!("queued turn expected")
    };
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn,
        active
    );
    let mut captured = Vec::new();
    for text in ["First waiting input", "Second waiting input"] {
        let Output::QueuedTurn(turn) = command(
            &mut fixture.database,
            &fixture.events,
            Command::SubmitTurn {
                session: fixture.session,
                expected_revision: 1,
                message: text.into(),
            },
        ) else {
            panic!("queued turn expected")
        };
        captured.push(message(&fixture.database.connection, turn.id).unwrap());
    }
    pause(&fixture.database.connection, fixture.session, true).unwrap();
    assert!(!can_resume(&fixture.database.connection, fixture.session).unwrap());
    pause(&fixture.database.connection, fixture.session, false).unwrap();
    runs::handoff(&fixture.database.connection, active.id).unwrap();
    assert!(!can_resume(&fixture.database.connection, fixture.session).unwrap());
    let Output::Run(stopped) = command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: active.id },
    ) else {
        panic!("run expected")
    };
    assert_eq!(stopped.status, Status::Stopping);
    assert!(
        read(&fixture.database.connection, fixture.session)
            .unwrap()
            .paused
    );
    let execute = |fixture: &mut Fixture, request: Request| {
        let (reply, response) = oneshot::channel();
        assert!(
            fixture
                .database
                .dispatch(fixture.database.node, request, reply, &fixture.events)
                .is_none()
        );
        response
            .blocking_recv()
            .unwrap()
            .unwrap()
            .completion
            .blocking_recv()
            .unwrap()
            .unwrap()
    };
    let request = Request::new(
        fixture.database.node,
        Command::SubmitTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Current composer input".into(),
        },
    );
    let Output::QueuedTurn(fresh) = execute(&mut fixture, request.clone()) else {
        panic!("queued turn expected")
    };
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(!queue.paused);
    assert_eq!(
        queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
        [fresh.id, captured[0].turn.id, captured[1].turn.id]
    );
    for before in &captured {
        assert_eq!(
            message(&fixture.database.connection, before.turn.id).unwrap(),
            *before
        );
    }
    pause(&fixture.database.connection, fixture.session, true).unwrap();
    let paused = read(&fixture.database.connection, fixture.session).unwrap();
    assert_eq!(
        execute(&mut fixture, request),
        Output::QueuedTurn(fresh.clone())
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session).unwrap(),
        paused
    );
    pause(&fixture.database.connection, fixture.session, false).unwrap();
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    runs::finish(
        &mut fixture.database,
        active.id,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    for (turn, input) in [
        (fresh, "Current composer input"),
        (captured[0].turn.clone(), "First waiting input"),
        (captured[1].turn.clone(), "Second waiting input"),
    ] {
        let invocation = runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, turn);
        assert_eq!(invocation.message.text, input);
        runs::finish(
            &mut fixture.database,
            turn.id,
            Status::Completed,
            None,
            &fixture.events,
        )
        .unwrap();
    }
}
