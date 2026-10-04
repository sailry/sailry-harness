use super::{fixture::Fixture, *};
use serde_json::json;

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
