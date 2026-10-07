use super::*;
use crate::agent::Controls;
use crate::store::agent::tests::fixture::Fixture;
use sailry_link::CancellationToken;

fn admitted(fixture: &mut Fixture) -> (TurnId, TurnId) {
    let active = fixture.queued();
    runs::start(&fixture.database.connection, active).unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .unwrap()
            .turn
            .id,
        active
    );
    let source = fixture.queued();
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
    .unwrap();
    (active, source)
}

fn handoff(fixture: &mut Fixture, active: TurnId) -> Controls {
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
    );
    assert!(response.try_recv().unwrap().unwrap());
    assert!(controls.yielded(active));
    controls
}

fn finish(
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
            reply,
        },
        &fixture.events,
        controls,
    );
    response.try_recv().unwrap().unwrap()
}

#[test]
fn stop_wins_after_handoff() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    let controls = handoff(&mut fixture, active);
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .unwrap()
            .status,
        Status::Stopping
    );
    assert!(
        !read(&fixture.database.connection, fixture.session)
            .unwrap()
            .paused
    );
    runs::stop(&fixture.database.connection, active).unwrap();
    controls.cancel(active);
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(!yield_run(&mut fixture.database, active, &fixture.events).unwrap());
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Cancelled,
            None,
            None
        ),
        Status::Cancelled
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    assert!(
        runs::claim_guidance(&mut fixture.database, active, &fixture.events)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        message(&fixture.database.connection, source)
            .unwrap()
            .message
            .text,
        "History fixture"
    );
    assert_eq!(
        runs::get(&fixture.database.connection, source)
            .unwrap()
            .status,
        Status::Queued
    );
}

#[test]
fn stop_overrides_observed_completion() {
    let mut fixture = Fixture::new();
    let (active, _) = admitted(&mut fixture);
    let controls = Controls::default();
    controls.register(active, fixture.session, CancellationToken::new());
    runs::stop(&fixture.database.connection, active).unwrap();
    controls.cancel(active);
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Completed,
            None,
            None
        ),
        Status::Cancelled
    );
}

#[test]
fn handoff_survives_queue_changes() {
    for (status, paused) in [
        (Status::Cancelled, false),
        (Status::Cancelled, true),
        (Status::Completed, false),
        (Status::Completed, true),
    ] {
        let mut fixture = Fixture::new();
        let (active, source) = admitted(&mut fixture);
        let controls = handoff(&mut fixture, active);
        if paused {
            pause(&fixture.database.connection, fixture.session, true).unwrap();
        } else {
            runs::stop(&fixture.database.connection, source).unwrap();
        }
        assert_eq!(
            finish(&mut fixture, &controls, active, status, None, None),
            Status::Continued
        );
        assert_eq!(
            runs::get(&fixture.database.connection, active)
                .unwrap()
                .status,
            Status::Continued
        );
        assert_eq!(
            read(&fixture.database.connection, fixture.session)
                .unwrap()
                .paused,
            paused
        );
    }
}

#[test]
fn owner_stop_overrides_handoff() {
    for (shutdown, observed, error) in [
        (false, Status::Cancelled, None),
        (true, Status::Cancelled, None),
        (
            true,
            Status::Failed,
            Some(Fault::new(
                ErrorCode::InvalidRequest,
                "Model request failed",
            )),
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
        let mut fixture = Fixture::new();
        let (active, source) = admitted(&mut fixture);
        let controls = handoff(&mut fixture, active);
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
            ),
            status
        );
        let run = runs::get(&fixture.database.connection, active).unwrap();
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
        let queue = read(&fixture.database.connection, fixture.session).unwrap();
        assert!(queue.paused);
        assert_eq!(queue.items[0].turn, source);
        assert_eq!(
            runs::get(&fixture.database.connection, source)
                .unwrap()
                .status,
            Status::Queued
        );
        assert!(
            runs::claim(&mut fixture.database, &fixture.events)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn preserves_finished_handoff_on_recovery() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    let controls = handoff(&mut fixture, active);
    assert_eq!(
        finish(
            &mut fixture,
            &controls,
            active,
            Status::Cancelled,
            None,
            None
        ),
        Status::Continued
    );
    runs::recover(&fixture.database.connection).unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .unwrap()
            .status,
        Status::Continued
    );
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}

#[test]
fn pause_blocks_boundary() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    pause(&fixture.database.connection, fixture.session, true).unwrap();
    assert!(!yield_run(&mut fixture.database, active, &fixture.events).unwrap());
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .unwrap()
            .status,
        Status::Running
    );
    assert_eq!(
        runs::get(&fixture.database.connection, source)
            .unwrap()
            .status,
        Status::Queued
    );
    assert!(
        read(&fixture.database.connection, fixture.session)
            .unwrap()
            .paused
    );
}

#[test]
fn recovers_unconsumed_intent() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    runs::recover(&fixture.database.connection).unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .unwrap()
            .status,
        Status::Interrupted
    );
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert_eq!(
        message(&fixture.database.connection, source)
            .unwrap()
            .message
            .text,
        "History fixture"
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}

#[test]
fn recovers_unfinished_handoff() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    let _controls = handoff(&mut fixture, active);
    runs::recover(&fixture.database.connection).unwrap();
    assert_eq!(
        runs::get(&fixture.database.connection, active)
            .unwrap()
            .status,
        Status::Interrupted
    );
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
}
