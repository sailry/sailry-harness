use super::*;

#[tokio::test]
async fn follows_current_session_title() {
    let mut fixture = Fixture::new().await;
    fixture.append([event(START, 10)]).await;
    let db = &fixture.database.connection;
    let derived = crate::store::agent::activity::title(db, fixture.session)
        .await
        .unwrap();
    assert!(!derived.is_empty());
    let title = async |fixture: &Fixture| {
        report(fixture, &query()).await.requests.items[0]
            .session_title
            .clone()
    };
    assert_eq!(title(&fixture).await, Some(derived));
    let revision = session(&mut fixture).await.revision;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RenameSession {
            session: fixture.session,
            expected_revision: revision,
            title: "User title".into(),
        },
    )
    .await;
    assert_eq!(title(&fixture).await.as_deref(), Some("User title"));
    fixture
        .database
        .connection
        .execute(
            "UPDATE agent_state SET body=?1 WHERE owner=?2 AND key=?3",
            params![
                serde_json::to_vec(&"   ").unwrap(),
                fixture.session.to_string(),
                crate::store::agent::SESSION_TITLE
            ],
        )
        .await
        .unwrap();
    assert_eq!(title(&fixture).await, None);
}

#[tokio::test]
async fn names_groups_by_dimension() {
    let mut fixture = Fixture::new().await;
    fixture.append([event(START, 10)]).await;
    let call = report(&fixture, &query()).await.requests.items[0].clone();
    for (dimension, name) in [
        (Dimension::Model, Some(call.provider_name.clone())),
        (Dimension::Provider, Some(call.provider_name.clone())),
        (Dimension::Project, Some("Turn history fixture".to_owned())),
        (Dimension::Worktree, None),
    ] {
        let mut selection = query();
        selection.dimension = dimension;
        let groups = report(&fixture, &selection).await.groups;
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, name, "{dimension:?}");
    }
    assert_eq!(call.scope_name, "Turn history fixture");
}
