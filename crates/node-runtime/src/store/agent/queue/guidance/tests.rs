use super::*;
use crate::agent::Controls;
use crate::store::agent::tests::fixture::Fixture;
use sailry_link::CancellationToken;

async fn admitted(fixture: &mut Fixture) -> (TurnId, TurnId) {
    let active = fixture.queued().await;
    runs::start(&fixture.database.connection, active)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, active);
    let mut event = AdkEvent::new(active.to_string());
    event.author = "user".into();
    event.set_content(adk_core::Content::new("user").with_text(&invocation.message.text));
    super::super::super::history::append(
        &mut fixture.database,
        fixture.session,
        active,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
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

async fn registered(fixture: &mut Fixture, active: TurnId) -> Controls {
    let controls = Controls::default();
    controls.register(active, fixture.session, CancellationToken::new());
    controls
}

async fn take(
    fixture: &mut Fixture,
    active: TurnId,
    controls: &Controls,
) -> Option<Vec<sailry_protocol::conversation::QueuedMessage>> {
    let (reply, mut response) = oneshot::channel();
    crate::store::agent::execute(
        &mut fixture.database,
        Operation::TakeGuidance {
            recipient: active,
            reply,
        },
        &fixture.events,
        controls,
    )
    .await;
    response.try_recv().unwrap().unwrap()
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
async fn admission_does_not_interrupt_the_recipient() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
    assert!(controls.running(active));
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
    assert_eq!(
        take(&mut fixture, active, &controls).await.unwrap()[0]
            .turn
            .id,
        source
    );
    assert!(controls.running(active));
}

#[tokio::test]
async fn resumes_without_ending_the_task() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
    let (reply, mut response) = oneshot::channel();
    crate::store::agent::execute(
        &mut fixture.database,
        Operation::TakeGuidance {
            recipient: active,
            reply,
        },
        &fixture.events,
        &controls,
    )
    .await;
    let inputs = response.try_recv().unwrap().unwrap().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].turn.id, source);
    let request = Request::new(
        fixture.database.node,
        Command::WithdrawGuidance {
            turn: source,
            expected_revision: 2,
        },
    );
    assert_eq!(
        super::super::commands::execute(
            &fixture.database.connection,
            fixture.database.node,
            &request,
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );

    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Running
    );
    let joined = runs::get(&fixture.database.connection, source)
        .await
        .unwrap();
    assert_eq!(joined.joined, Some(active));
    assert_eq!(joined.status, Status::Continued);
    assert!(controls.running(active));
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn restores_only_unconsumed_inputs() {
    for consumed in [false, true] {
        let mut fixture = Fixture::new().await;
        let (active, source) = admitted(&mut fixture).await;
        let message = super::super::message(&fixture.database.connection, source)
            .await
            .unwrap();
        let controls = registered(&mut fixture, active).await;
        let (reply, mut response) = oneshot::channel();
        crate::store::agent::execute(
            &mut fixture.database,
            Operation::TakeGuidance {
                recipient: active,
                reply,
            },
            &fixture.events,
            &controls,
        )
        .await;
        response.try_recv().unwrap().unwrap().unwrap();
        if consumed {
            let mut event = AdkEvent::new(active.to_string());
            event.author = "user".into();
            event.set_content(adk_core::Content::new("user").with_text(&message.message.text));
            super::super::super::history::append(
                &mut fixture.database,
                fixture.session,
                source,
                event,
                &fixture.events,
            )
            .await
            .unwrap();
        }
        assert_eq!(
            finish(
                &mut fixture,
                &controls,
                active,
                Status::Failed,
                Some(Fault::new(
                    ErrorCode::Unavailable,
                    "task preparation failed"
                )),
                None
            )
            .await,
            Status::Failed
        );
        let queue = read(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
        let run = runs::get(&fixture.database.connection, source)
            .await
            .unwrap();
        if consumed {
            assert!(queue.items.is_empty());
            assert_eq!(run.joined, Some(active));
        } else {
            assert!(queue.paused);
            assert_eq!(queue.items.len(), 1);
            assert_eq!(queue.items[0].turn, source);
            assert_eq!(queue.items[0].guidance, None);
            assert!(!queue.items[0].ready);
            assert_eq!(run.joined, None);
            assert_eq!(run.status, Status::Queued);
            assert_eq!(
                super::super::message(&fixture.database.connection, source)
                    .await
                    .unwrap()
                    .message,
                message.message
            );
        }
    }
}

#[tokio::test]
async fn stop_prevents_claimed_input_from_being_consumed() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
    let inputs = take(&mut fixture, active, &controls).await.unwrap();
    runs::stop(&fixture.database.connection, active)
        .await
        .unwrap();
    controls.cancel(active);
    let mut event = AdkEvent::new(active.to_string());
    event.author = "user".into();
    event.set_content(adk_core::Content::new("user").with_text(&inputs[0].message.text));
    let error = super::super::super::history::append(
        &mut fixture.database,
        fixture.session,
        source,
        event,
        &fixture.events,
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("guidance recipient is no longer running")
    );
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
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(
        runs::get(&fixture.database.connection, source)
            .await
            .unwrap()
            .joined,
        None
    );
}

#[tokio::test]
async fn retains_preparation_failure_for_explicit_retry() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        runs::finish(
            &mut fixture.database,
            turn,
            Status::Failed,
            Some(Fault::new(
                ErrorCode::Unavailable,
                "context preparation failed"
            )),
            &fixture.events
        )
        .await
        .unwrap(),
        Status::Failed
    );
    let queue = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].turn, turn);
    assert!(!queue.items[0].ready);
    assert_eq!(
        super::super::message(&fixture.database.connection, turn)
            .await
            .unwrap()
            .message,
        invocation.message
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .is_none()
    );
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .paused
    );
    let retried = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.turn, invocation.turn);
    assert_eq!(retried.message, invocation.message);
    let mut event = AdkEvent::new(turn.to_string());
    event.author = "user".into();
    event.set_content(adk_core::Content::new("user").with_text(&retried.message.text));
    super::super::super::history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Failed,
        Some(Fault::new(ErrorCode::Unavailable, "request failed")),
        &fixture.events,
    )
    .await
    .unwrap();
    assert!(
        read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        runs::start(&fixture.database.connection, turn)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn stop_wins_before_the_boundary() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .await
            .unwrap()
            .status,
        Status::Running
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
    assert!(take(&mut fixture, active, &controls).await.is_none());
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
async fn queue_changes_do_not_replace_the_owner() {
    for paused in [false, true] {
        let mut fixture = Fixture::new().await;
        let (active, source) = admitted(&mut fixture).await;
        let controls = registered(&mut fixture, active).await;
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
            finish(
                &mut fixture,
                &controls,
                active,
                Status::Completed,
                None,
                None
            )
            .await,
            Status::Completed
        );
        assert_eq!(
            runs::get(&fixture.database.connection, active)
                .await
                .unwrap()
                .status,
            Status::Completed
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
async fn owner_stop_retains_pending_input() {
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
        let controls = registered(&mut fixture, active).await;
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
async fn preserves_completed_owner_on_recovery() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
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
        Status::Completed
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
        Status::Completed
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
        runs::guidance::take(&mut fixture.database, active, &fixture.events)
            .await
            .unwrap()
            .is_none()
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
async fn recovers_active_owner() {
    let mut fixture = Fixture::new().await;
    let (active, source) = admitted(&mut fixture).await;
    let controls = registered(&mut fixture, active).await;
    assert_eq!(
        take(&mut fixture, active, &controls).await.unwrap()[0]
            .turn
            .id,
        source
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
    async fn joins_selected_inputs_at_the_boundary() {
        let mut fixture = Fixture::new().await;
        let (active, first) = admitted(&mut fixture).await;
        let waiting = fixture.queued().await;
        let second = guide(&mut fixture, active, "Keep the tests").await;
        let controls = registered(&mut fixture, active).await;
        let inputs = take(&mut fixture, active, &controls).await.unwrap();
        assert_eq!(
            inputs.iter().map(|input| input.turn.id).collect::<Vec<_>>(),
            [first, second]
        );
        for input in inputs {
            let run = runs::get(&fixture.database.connection, input.turn.id)
                .await
                .unwrap();
            assert_eq!(run.joined, Some(active));
            assert_eq!(run.status, Status::Continued);
        }
        assert_eq!(
            runs::get(&fixture.database.connection, active)
                .await
                .unwrap()
                .status,
            Status::Running
        );
        assert_eq!(
            read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .items
                .iter()
                .map(|item| item.turn)
                .collect::<Vec<_>>(),
            [waiting]
        );
    }

    #[tokio::test]
    async fn accepts_later_input_in_the_same_invocation() {
        let mut fixture = Fixture::new().await;
        let (active, first) = admitted(&mut fixture).await;
        let controls = registered(&mut fixture, active).await;
        assert_eq!(
            take(&mut fixture, active, &controls).await.unwrap()[0]
                .turn
                .id,
            first
        );
        let last = guide(&mut fixture, active, "Later boundary input").await;
        let inputs = take(&mut fixture, active, &controls).await.unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].turn.id, last);
        assert_eq!(inputs[0].message.text, "Later boundary input");
        assert!(take(&mut fixture, active, &controls).await.is_none());
        assert!(controls.running(active));
    }
}
