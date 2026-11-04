use super::fixture::record;
use super::{fixture::Fixture, *};
use crate::agent::canonical::{Configuration, Context, Kind};

#[tokio::test]
async fn preserves_title_ownership() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "captures_assigned_title_at_execution",
        "shares_title_and_preserves_it_when_rebuilding",
        "truncates_title_projection_without_changing_source",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "captures_assigned_title_at_execution" => {
                Box::pin(async {
                    let first = fixture.queued().await;
                    runs::start(&fixture.database.connection, first)
                        .await
                        .unwrap();
                    let invocation = runs::claim(&mut fixture.database, &fixture.events)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(invocation.turn.id, first, "{scenario}");
                    assert!(invocation.title.is_none(), "{scenario}");
                    assert!(
                        !activity::title(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()
                            .is_empty(),
                        "{scenario}"
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
                    assert_eq!(invocation.turn, input.turn, "{scenario}");
                    assert_eq!(invocation.message, input.message, "{scenario}");
                    assert_eq!(
                        invocation.title.as_deref(),
                        Some("User title"),
                        "{scenario}"
                    );
                    assert_eq!(
                        history::state(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()[SESSION_TITLE],
                        "User title",
                        "{scenario}"
                    );
                })
                .await
            }
            "shares_title_and_preserves_it_when_rebuilding" => {
                Box::pin(async {
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
                        "Agent title",
                        "{scenario}"
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
                        "User title",
                        "{scenario}"
                    );
                    history::rebuild(&mut fixture.database.connection)
                        .await
                        .unwrap();
                    assert_eq!(
                        activity::read(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()
                            .title,
                        "User title",
                        "{scenario}"
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
                        "Updated title",
                        "{scenario}"
                    );
                    history::rebuild(&mut fixture.database.connection)
                        .await
                        .unwrap();
                    assert_eq!(
                        activity::read(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()
                            .title,
                        "Updated title",
                        "{scenario}"
                    );
                })
                .await
            }
            "truncates_title_projection_without_changing_source" => {
                Box::pin(async {
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
                    assert!(projected.chars().count() <= 48, "{scenario}");
                    assert!(projected.ends_with('…'), "{scenario}");
                    assert_eq!(
                        history::state(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()[SESSION_TITLE],
                        original,
                        "{scenario}"
                    );
                    history::rebuild(&mut fixture.database.connection)
                        .await
                        .unwrap();
                    assert_eq!(
                        activity::read(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()
                            .title,
                        projected,
                        "{scenario}"
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
                        projected,
                        "{scenario}"
                    );
                    assert_eq!(
                        history::state(&fixture.database.connection, fixture.session)
                            .await
                            .unwrap()[SESSION_TITLE],
                        original.trim(),
                        "{scenario}"
                    );
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}
