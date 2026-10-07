use super::*;
use crate::store::agent::tests::fixture::Fixture;

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

#[test]
fn stop_wins_after_handoff() {
    let mut fixture = Fixture::new();
    let (active, source) = admitted(&mut fixture);
    assert!(yield_run(&mut fixture.database, active, &fixture.events).unwrap());
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
    let queue = read(&fixture.database.connection, fixture.session).unwrap();
    assert!(queue.paused);
    assert_eq!(queue.items[0].turn, source);
    assert_eq!(queue.items[0].guidance, None);
    assert!(!yield_run(&mut fixture.database, active, &fixture.events).unwrap());
    runs::finish(
        &mut fixture.database,
        active,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
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
