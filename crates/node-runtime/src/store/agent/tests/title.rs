use super::{fixture::Fixture, *};
use serde_json::json;

#[test]
fn captures_assigned_title_at_execution() {
    let mut fixture = Fixture::new();
    let first = fixture.queued();
    runs::start(&fixture.database.connection, first).unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, first);
    assert!(invocation.title.is_none());
    assert!(
        !activity::title(&fixture.database.connection, fixture.session)
            .unwrap()
            .is_empty()
    );
    runs::finish(
        &mut fixture.database,
        first,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();

    let second = fixture.queued();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RenameSession {
            session: fixture.session,
            expected_revision: 1,
            title: "User title".into(),
        },
    );
    history::rebuild_session(&fixture.database.connection, fixture.session).unwrap();
    let input = queue::message(&fixture.database.connection, second).unwrap();
    runs::start(&fixture.database.connection, second).unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn, input.turn);
    assert_eq!(invocation.message, input.message);
    assert_eq!(invocation.title.as_deref(), Some("User title"));
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session).unwrap()[SESSION_TITLE],
        "User title"
    );
}

#[test]
fn shares_title_and_preserves_it_when_rebuilding() {
    let mut fixture = Fixture::new();
    let mut event = AdkEvent::new("named-turn");
    event.author = "assistant".into();
    event
        .actions
        .state_delta
        .insert(SESSION_TITLE.into(), json!("Agent title"));
    fixture.append([event]);
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        "Agent title"
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RenameSession {
            session: fixture.session,
            expected_revision: 1,
            title: "User title".into(),
        },
    );
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session).unwrap()[SESSION_TITLE],
        "User title"
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        "User title"
    );
    // A subsequent explicit agent rename updates that same field, with no manual override.
    let mut event = AdkEvent::new("renamed-turn");
    event.author = "assistant".into();
    event
        .actions
        .state_delta
        .insert(SESSION_TITLE.into(), json!("Updated title"));
    fixture.append([event]);
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        "Updated title"
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        "Updated title"
    );
}

#[test]
fn truncates_title_projection_without_changing_source() {
    let mut fixture = Fixture::new();
    let original = "A long conversation title ".repeat(10);
    let mut event = AdkEvent::new("named-turn");
    event.author = "assistant".into();
    event
        .actions
        .state_delta
        .insert(SESSION_TITLE.into(), json!(original));
    fixture.append([event]);
    let projected = activity::read(&fixture.database.connection, fixture.session)
        .unwrap()
        .title;
    assert!(projected.chars().count() <= 48);
    assert!(projected.ends_with('…'));
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session).unwrap()[SESSION_TITLE],
        original
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        projected
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RenameSession {
            session: fixture.session,
            expected_revision: 1,
            title: original.clone(),
        },
    );
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .unwrap()
            .title,
        projected
    );
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session).unwrap()[SESSION_TITLE],
        original.trim()
    );
}
