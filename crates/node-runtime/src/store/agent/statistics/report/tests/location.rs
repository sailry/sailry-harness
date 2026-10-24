use super::*;

#[tokio::test]
async fn attributes_usage_to_admitted_location() {
    let mut fixture = Fixture::new().await;
    let source = session(&mut fixture).await;
    fixture.append([event(START, 10)]).await;
    let directory = tempfile::tempdir().unwrap();
    let target = sailry_protocol::Worktree {
        id: WorktreeId::new(),
        project: source.project,
        path: directory.path().to_str().unwrap().into(),
        main: false,
    };
    crate::store::worktrees::register(&fixture.database.connection, &target)
        .await
        .unwrap();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::MoveConversation {
            session: source.id,
            worktree: target.id,
            expected_revision: 1,
        },
    )
    .await;
    fixture.append([event(START + DAY, 20)]).await;
    for (worktree, input) in [(source.worktree, 10), (target.id, 20)] {
        let mut filtered = query();
        filtered.worktrees = vec![worktree];
        assert_eq!(
            report(&fixture, &filtered)
                .await
                .totals
                .tokens
                .unwrap()
                .input,
            input
        );
    }
    assert_eq!(
        report(&fixture, &query())
            .await
            .totals
            .tokens
            .unwrap()
            .input,
        30
    );
}

#[tokio::test]
async fn attributes_unassigned_sessions() {
    let mut fixture = Fixture::new().await;
    let config = session(&mut fixture).await.config;
    fixture
        .database
        .set_profile(Some(fixture._root.path().canonicalize().unwrap()))
        .unwrap();
    let Output::Session(session) = command(
        &mut fixture.database,
        &fixture.events,
        Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(config),
        },
    )
    .await
    else {
        panic!("session expected");
    };
    fixture.session = session.id;
    let turn = fixture.append([event(START, 10)]).await;
    let usage = report(&fixture, &query()).await;
    let tokens = Usage {
        input: 10,
        output: 4,
        cached_input: 2,
        reasoning: 1,
    };
    assert_eq!(usage.totals.responses, 1);
    assert_eq!(usage.totals.tokens, Some(tokens.clone()));
    assert_eq!(
        usage.resources,
        [
            Key::Provider(session.config.provider),
            Key::Model {
                provider: session.config.provider,
                model: session.config.model.clone(),
            },
            Key::Session(session.id),
        ]
    );
    assert_eq!(usage.groups.len(), 1);
    assert_eq!(
        usage.groups[0].key,
        Key::Model {
            provider: session.config.provider,
            model: session.config.model.clone(),
        }
    );
    let request = &usage.requests.items[0];
    assert_eq!(request.session, session.id);
    assert_eq!(request.turn, turn);
    assert_eq!(request.project, None);
    assert_eq!(request.worktree, session.worktree);
    assert_eq!(request.scope_name, "");
    assert_eq!(request.tokens, tokens);
    for dimension in [Dimension::Project, Dimension::Worktree] {
        let mut filtered = query();
        filtered.dimension = dimension;
        let grouped = report(&fixture, &filtered).await;
        assert_eq!(grouped.groups.len(), 1);
        assert_eq!(grouped.groups[0].key, Key::Session(session.id));
        assert_eq!(grouped.totals, usage.totals);
    }
}
