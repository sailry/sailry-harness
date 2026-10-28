use super::fixture::record;
use super::{fixture::Fixture, *};
use crate::agent::canonical::{Configuration, Context, Kind};

#[tokio::test]
async fn captures_assigned_title_at_execution() {
    let mut fixture = Fixture::new().await;
    let first = fixture.queued().await;
    runs::start(&fixture.database.connection, first)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, first);
    assert!(invocation.title.is_none());
    assert!(
        !activity::title(&fixture.database.connection, fixture.session)
            .await
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
    .await
    .unwrap();

    let second = fixture.queued().await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RenameSession {
            session: fixture.session,
            expected_revision: 1,
            title: "User title".into(),
        },
    )
    .await;
    history::rebuild_session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let input = queue::message(&fixture.database.connection, second)
        .await
        .unwrap();
    runs::start(&fixture.database.connection, second)
        .await
        .unwrap();
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn, input.turn);
    assert_eq!(invocation.message, input.message);
    assert_eq!(invocation.title.as_deref(), Some("User title"));
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[SESSION_TITLE],
        "User title"
    );
}

#[tokio::test]
async fn shares_title_and_preserves_it_when_rebuilding() {
    let mut fixture = Fixture::new().await;
    let event = record(
        "assistant",
        Kind::Configuration(Configuration {
            context: Context {
                title: Some("Agent title".into()),
                ..Default::default()
            },
        }),
    );
    fixture.append([event]).await;
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
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
    )
    .await;
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[SESSION_TITLE],
        "User title"
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .title,
        "User title"
    );
    // A subsequent explicit agent rename updates that same field, with no manual override.
    let event = record(
        "assistant",
        Kind::Configuration(Configuration {
            context: Context {
                title: Some("Updated title".into()),
                ..Default::default()
            },
        }),
    );
    fixture.append([event]).await;
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .title,
        "Updated title"
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .title,
        "Updated title"
    );
}

#[tokio::test]
async fn truncates_title_projection_without_changing_source() {
    let mut fixture = Fixture::new().await;
    let original = "A long conversation title ".repeat(10);
    let event = record(
        "assistant",
        Kind::Configuration(Configuration {
            context: Context {
                title: Some(original.clone()),
                ..Default::default()
            },
        }),
    );
    fixture.append([event]).await;
    let projected = activity::read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .title;
    assert!(projected.chars().count() <= 48);
    assert!(projected.ends_with('…'));
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[SESSION_TITLE],
        original
    );
    history::rebuild(&mut fixture.database.connection)
        .await
        .unwrap();
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
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
    )
    .await;
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .title,
        projected
    );
    assert_eq!(
        history::state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[SESSION_TITLE],
        original.trim()
    );
}
