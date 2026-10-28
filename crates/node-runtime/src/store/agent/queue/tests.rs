use super::*;
use crate::agent::canonical::{self, BlockKind};
use crate::store::agent::tests::{
    command,
    fixture::{self, Fixture},
};

#[tokio::test]
async fn pauses_unclaimed_input_after_restart() {
    for ready in [false, true] {
        let mut fixture = Fixture::new().await;
        let turn = fixture.queued().await;
        if ready {
            runs::start(&fixture.database.connection, turn)
                .await
                .unwrap();
        }
        let message = message(&fixture.database.connection, turn).await.unwrap();
        let before = read(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
        assert!(!before.paused);
        assert_eq!(before.items[0].ready, ready);
        assert_eq!(before.items[0].request, Some(message.turn.request));
        let path = fixture
            ._root
            .path()
            .canonicalize()
            .unwrap()
            .join("node.turso");
        let node = fixture.database.node;
        fixture.database.close().await.unwrap();
        let mut database = Database::open(&path, node, None).await.unwrap();
        let recovered = read(&database.connection, fixture.session).await.unwrap();
        assert!(recovered.paused);
        assert_eq!(recovered.revision, before.revision + 1);
        assert_eq!(recovered.items, before.items);
        assert_eq!(
            super::message(&database.connection, turn).await.unwrap(),
            message
        );
        assert_eq!(
            runs::get(&database.connection, turn).await.unwrap().status,
            Status::Queued
        );
        assert!(
            runs::claim(&mut database, &fixture.events)
                .await
                .unwrap()
                .is_none()
        );

        database.close().await.unwrap();
        let mut database = Database::open(&path, node, None).await.unwrap();
        assert_eq!(
            read(&database.connection, fixture.session).await.unwrap(),
            recovered
        );
        runs::start(&database.connection, turn).await.unwrap();
        assert!(
            runs::claim(&mut database, &fixture.events)
                .await
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
        )
        .await
        else {
            panic!("queued turn expected")
        };
        let invocation = runs::claim(&mut database, &fixture.events)
            .await
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
        .await
        .unwrap();
        let invocation = runs::claim(&mut database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, message.turn);
        assert_eq!(invocation.message, message.message);
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn edits_publish_current_input_identity() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    let original = message(&fixture.database.connection, turn).await.unwrap();
    let before = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(before.items[0].request, Some(original.turn.request));
    let Output::Queue(update) = command(
        &mut fixture.database,
        &fixture.events,
        Command::EditQueuedTurn {
            turn,
            expected_revision: before.items[0].revision,
            message: original.message.clone(),
        },
    )
    .await
    else {
        panic!("queue update expected");
    };
    let edited = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(edited, update.queue);
    assert_eq!(edited.items[0].revision, before.items[0].revision + 1);
    assert_ne!(edited.items[0].request, before.items[0].request);
    let stored: String = fixture
        .database
        .connection
        .prepare("SELECT request FROM agent_pending WHERE turn=?1")
        .await
        .unwrap()
        .query_row([turn.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(edited.items[0].request, Some(stored.parse().unwrap()));
    assert_eq!(
        message(&fixture.database.connection, turn)
            .await
            .unwrap()
            .turn,
        original.turn
    );
}

#[tokio::test]
async fn fresh_input_precedes_stopped_queue() {
    let mut fixture = Fixture::new().await;
    let Output::QueuedTurn(active) = command(
        &mut fixture.database,
        &fixture.events,
        Command::SubmitTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Active input".into(),
        },
    )
    .await
    else {
        panic!("queued turn expected")
    };
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn,
        active
    );
    let mut event = fixture::message(
        "user",
        canonical::Role::User,
        vec![BlockKind::Text {
            text: "Active input".into(),
        }],
    );
    event.turn = active.id;
    history::append(
        &mut fixture.database,
        fixture.session,
        active.id,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
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
        )
        .await
        else {
            panic!("queued turn expected")
        };
        captured.push(
            message(&fixture.database.connection, turn.id)
                .await
                .unwrap(),
        );
    }
    pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    assert!(
        !can_resume(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
    );
    pause(&fixture.database.connection, fixture.session, false)
        .await
        .unwrap();
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused,
        "queue did not resume before Stop"
    );
    runs::stop(&fixture.database.connection, active.id)
        .await
        .unwrap();
    assert!(
        can_resume(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
    );
    let Output::Run(stopped) = command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: active.id },
    )
    .await
    else {
        panic!("run expected")
    };
    assert_eq!(stopped.status, Status::Stopping);
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused
    );
    let execute = async |fixture: &mut Fixture, request: Request| {
        let (reply, response) = oneshot::channel();
        assert!(
            fixture
                .database
                .dispatch(fixture.database.node, request, reply, &fixture.events)
                .await
                .is_none()
        );
        response
            .await
            .unwrap()
            .unwrap()
            .completion
            .await
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
    let Output::QueuedTurn(fresh) = execute(&mut fixture, request.clone()).await else {
        panic!("queued turn expected")
    };
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(!queue.paused);
    assert_eq!(
        queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
        [fresh.id, captured[0].turn.id, captured[1].turn.id]
    );
    for before in &captured {
        assert_eq!(
            message(&fixture.database.connection, before.turn.id)
                .await
                .unwrap(),
            *before
        );
    }
    pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    let paused = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(
        execute(&mut fixture, request).await,
        Output::QueuedTurn(fresh.clone())
    );
    assert_eq!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap(),
        paused
    );
    assert!(fixture.database.connection.is_autocommit().unwrap());
    pause(&fixture.database.connection, fixture.session, false)
        .await
        .unwrap();
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused,
        "queue did not resume after replay"
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused,
        "empty claim changed queue pause state"
    );
    runs::finish(
        &mut fixture.database,
        active.id,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    for (turn, input) in [
        (fresh, "Current composer input"),
        (captured[0].turn.clone(), "First waiting input"),
        (captured[1].turn.clone(), "Second waiting input"),
    ] {
        let invocation = runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(invocation.turn, turn);
        assert_eq!(invocation.message.text, input);
        let mut event = fixture::message(
            "user",
            canonical::Role::User,
            vec![BlockKind::Text { text: input.into() }],
        );
        event.turn = turn.id;
        history::append(
            &mut fixture.database,
            fixture.session,
            turn.id,
            event,
            &fixture.events,
        )
        .await
        .unwrap();
        runs::finish(
            &mut fixture.database,
            turn.id,
            Status::Completed,
            None,
            &fixture.events,
        )
        .await
        .unwrap();
    }
}
