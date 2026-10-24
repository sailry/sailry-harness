use super::*;
use crate::agent::Controls;
use crate::store::agent::tests::fixture::Fixture;
use sailry_link::CancellationToken;

async fn admitted(fixture: &mut Fixture) -> (TurnId, TurnId) {
    let active = fixture.queued().await;
    runs::start(&fixture.database.connection, active)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        active
    );
    let source = fixture.queued().await;
    let request = Request::new(
        fixture.database.node,
        Command::GuideQueuedTurn {
            turn: source,
            recipient: active,
            expected_revision: 1,
        },
    );
    select(
        &fixture.database.connection,
        fixture.database.node,
        &request,
        source,
        active,
        1,
    )
    .await
    .unwrap();
    (active, source)
}

async fn handoff(fixture: &mut Fixture, active: TurnId) -> Controls {
    let controls = Controls::default();
    controls.register(active, fixture.session, CancellationToken::new());
    let (reply, mut response) = oneshot::channel();
    crate::store::agent::execute(
        &mut fixture.database,
        Operation::YieldGuidance {
            recipient: active,
            reply,
        },
        &fixture.events,
        &controls,
    )
    .await;
    assert!(response.try_recv().unwrap().unwrap());
    assert!(controls.yielded(active));
    controls
}

async fn finish(
    fixture: &mut Fixture,
    controls: &Controls,
    active: TurnId,
    status: Status,
    error: Option<Fault>,
    owner: Option<CancellationToken>,
) -> Status {
    let (reply, mut response) = oneshot::channel();
    crate::store::agent::execute(
        &mut fixture.database,
        Operation::Finish {
            turn: active,
            status,
            error,
            owner,
            review: None,
            reply,
        },
        &fixture.events,
        controls,
    )
    .await;
    response.try_recv().unwrap().unwrap()
}

#[tokio::test]
async fn interrupts_before_callback_resumes() {
    let mut fixture = Fixture::new().await;
    let active = fixture.queued().await;
    runs::start(&fixture.database.connection, active)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let controls = Controls::default();
    let stop = CancellationToken::new();
    controls.register(active, fixture.session, stop.clone());
    let (reply, mut response) = oneshot::channel();
    crate::store::agent::execute(
        &mut fixture.database,
        Operation::YieldGuidance {
            recipient: active,
            reply,
        },
        &fixture.events,
        &controls,
    )
    .await;
    // Admit guidance before the waiting callback receives its empty-queue result.
    let source = fixture.queued().await;
    let request = Request::new(
        fixture.database.node,
        Command::GuideQueuedTurn {
            turn: source,
            recipient: active,
            expected_revision: 1,
        },
    );
    select(
        &fixture.database.connection,
        fixture.database.node,
        &request,
        source,
        active,
        1,
    )
    .await
    .unwrap();
    assert!(
        controls
            .interrupt_model(active, async || {
                yield_run(&mut fixture.database, active, &fixture.events).await
            })
            .await
            .unwrap()
    );
    assert!(stop.is_cancelled() && controls.yielded(active));
    assert!(!response.try_recv().unwrap().unwrap());
    assert!(!controls.model_settled(active).await);
}

#[tokio::test]
async fn stop_wins_after_handoff() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = handoff(&mut fixture, active).await;
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Stopping
    );
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused
    );
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    controls.cancel(active);
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        !yield_run(&mut fixture.database, active, &fixture.events)
            .await
            .unwrap()
    );
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Cancelled,
            None,
            None
        )
        .await,
        Status::Cancelled
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runs::claim_guidance(&mut fixture.database, active, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        message(&fixture.database.connection, source)
            .await
            .unwrap()
            .message
            .text,
        "History fixture"
    );
    assert_eq!(
        runs::get(&fixture.database.connection, source)
            .await
            .unwrap()
            .status,
        Status::Queued
    );
}

#[tokio::test]
async fn stop_overrides_observed_completion() {
    let mut fixture = Fixture::new().await;
    let (active, _) = admitted(&mut fixture).await;
    let controls = Controls::default();
    controls.register(active, fixture.session, CancellationToken::new());
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    controls.cancel(active);
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Completed,
            None,
            None
        )
        .await,
        Status::Cancelled
    );
}

#[tokio::test]
async fn handoff_survives_queue_changes() {
    for (status, paused) in [
        (Status::Cancelled, false),
        (Status::Cancelled, true),
        (Status::Completed, false),
        (Status::Completed, true),
    ] {
        let mut fixture = Fixture::new().await;
        let (active, source) = admitted(&mut fixture).await;
        let controls = handoff(&mut fixture, active).await;
        if paused {
            pause(&fixture.database.connection, fixture.session, true)
                .await
                .unwrap();
        } else {
            runs::stop(&fixture.database.connection, source)
                .await
                .unwrap();
        }
        assert_eq!(
            finish(&mut fixture, &controls, active, status, None, None).await,
            Status::Continued
        );
        assert_eq!(
            runs::get(&fixture.database.connection, active)
                .await
                .unwrap()
                .status,
            Status::Continued
        );
        assert_eq!(
            read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .paused,
            paused
        );
    }
}

#[tokio::test]
async fn owner_stop_overrides_handoff() {
    for (shutdown, observed, error) in [
        (false, Status::Cancelled, None),
        (true, Status::Cancelled, None),
        (
            true,
            Status::Failed,
            Some(Fault::invalid("Model request failed")),
        ),
        (
            true,
            Status::Failed,
            Some(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Detailed uncertain effect",
            )),
        ),
    ] {
        let mut fixture = Fixture::new().await;
        let (active, source) = admitted(&mut fixture).await;
        let controls = handoff(&mut fixture, active).await;
        let owner = CancellationToken::new();
        if shutdown {
            controls.stop();
        } else {
            owner.cancel();
        }
        let status = if shutdown {
            Status::Interrupted
        } else {
            Status::Cancelled
        };
        assert_eq!(
            finish(
                &mut fixture,
                &controls,
                active,
                observed,
                error.clone(),
                Some(owner),
            )
            .await,
            status
        );
        let run = runs::get(&fixture.database.connection, active)
            .await
            .unwrap();
        assert_eq!(run.status, status);
        if shutdown {
            let saved = run.error.unwrap();
            assert_eq!(saved.code, ErrorCode::OutcomeUnknown);
            if let Some(error) = error.filter(|error| error.code == ErrorCode::OutcomeUnknown) {
                assert_eq!(saved, error);
            } else {
                assert_eq!(
                    saved.message,
                    "Node stopped execution; calls were not replayed"
                );
            }
        }
        let queue = read(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
        assert!(queue.paused);
        assert_eq!(queue.items[0].turn, source);
        assert_eq!(
            runs::get(&fixture.database.connection, source)
                .await
                .unwrap()
                .status,
            Status::Queued
        );
        assert!(
            runs::claim(&mut fixture.database, &fixture.events)
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn preserves_finished_handoff_on_recovery() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = handoff(&mut fixture, active).await;
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Cancelled,
            None,
            None
        )
        .await,
        Status::Continued
    );
    {
        let transaction = turso::transaction::Transaction::new_unchecked(
            &fixture.database.connection,
            turso::transaction::TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    };
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Continued
    );
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn pause_blocks_boundary() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    pause(&fixture.database.connection, fixture.session, true)
        .await
        .unwrap();
    assert!(
        !yield_run(&mut fixture.database, active, &fixture.events)
            .await
            .unwrap()
    );
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Running
    );
    assert_eq!(
        runs::get(&fixture.database.connection, source)
            .await
            .unwrap()
            .status,
        Status::Queued
    );
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused
    );
}

#[tokio::test]
async fn recovers_unconsumed_intent() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    {
        let transaction = turso::transaction::Transaction::new_unchecked(
            &fixture.database.connection,
            turso::transaction::TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    };
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Interrupted
    );
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert_eq!(
        message(&fixture.database.connection, source)
            .await
            .unwrap()
            .message
            .text,
        "History fixture"
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn recovers_unfinished_handoff() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let _controls = handoff(&mut fixture, active).await;
    {
        let transaction = turso::transaction::Transaction::new_unchecked(
            &fixture.database.connection,
            turso::transaction::TransactionBehavior::Concurrent,
        )
        .await
        .unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    };
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Interrupted
    );
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
}

mod controller {
    use super::*;

    async fn guide(fixture: &mut Fixture, active: TurnId, text: &str) -> TurnId {
        let revision = runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .revision;
        let Output::QueuedTurn(turn) = crate::store::agent::tests::command(
            &mut fixture.database,
            &fixture.events,
            Command::GuideTurn {
                recipient: active,
                expected_revision: revision,
                message: text.into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        turn.id
    }

    #[tokio::test]
    async fn hands_off_at_boundary() {
        let mut fixture = Fixture::new().await;
        let active = fixture.queued().await;
        runs::start(&fixture.database.connection, active)
            .await
            .unwrap();
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        let waiting = fixture.queued().await;
        let first = guide(&mut fixture, active, "Use the second file").await;
        let second = guide(&mut fixture, active, "Keep the tests").await;
        let order: Vec<_> = read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .iter()
            .map(|item| (item.turn, item.guidance))
            .collect();
        assert_eq!(
            order,
            [(first, Some(active)), (second, None), (waiting, None)]
        );
        let controls = handoff(&mut fixture, active).await;
        assert_eq!(
            finish(
                &mut fixture,
                &controls,
                active,
                Status::Completed,
                None,
                None
            )
            .await,
            Status::Continued
        );
        let next = runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.turn.id, first);
        assert_eq!(next.message.text, "Use the second file");
    }
}
