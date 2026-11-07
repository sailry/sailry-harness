use super::*;
use sailry_protocol::conversation::{Association, Execution, Start};

fn draft(session: &Session) -> Start {
    Start {
        project: session.project,
        worktree: Some(session.worktree),
        config: Some(session.config.clone()),
        title: "Execution fixture".into(),
        message: "Complete the task".into(),
        execution: Execution::Model,
        association: Some(Association {
            namespace: "external".into(),
            kind: "task".into(),
            id: "daily".into(),
        }),
        team: None,
        models: Default::default(),
    }
}

async fn read(client: &Client, id: SessionId) -> Session {
    let Output::Session(session) = execute(client, Command::ReadSession { session: id }).await
    else {
        panic!("session expected")
    };
    session
}

async fn start(client: &Client, draft: Start) -> QueuedTurn {
    let Output::QueuedTurn(turn) = execute(client, Command::StartSession(draft)).await else {
        panic!("turn expected")
    };
    turn
}

#[tokio::test]
async fn freezes_selection_and_lead_atomically() {
    let (directory, node, controller, client) = fixture().await;
    let initial = provider(&client, None).await;
    let lead = provider(&client, Some("team-fixture")).await;
    let member = put(&client, profile("review"), 0).await.unwrap();
    let session = create(&client, directory.path(), &initial).await;
    let mut team = team("reviewers", vec![member.id]);
    team.model = Some(role::Model {
        provider: lead.id,
        model: "fixture".into(),
        effort: None,
        features: None,
    });
    let team = save(&client, team, 0).await.unwrap();
    let mut draft = draft(&session);
    draft.execution = Execution::Team;
    draft.team = Some(team::Selection {
        id: team.id,
        revision: None,
    });
    let request = client.prepare(Command::StartSession(draft.clone()));
    let first = client.execute(request.clone()).await.unwrap();
    let Output::QueuedTurn(first_turn) = &first else {
        panic!("turn expected")
    };
    assert_eq!(first_turn.config.provider, lead.id);
    assert_eq!(first_turn.config.credential, lead.credential);
    assert_eq!(first_turn.config.effort, Effort::High);
    assert_eq!(first_turn.roles.team, Some(team.reference()));
    assert_eq!(first_turn.roles.profiles, [member]);

    let mut updated = team.clone();
    updated.members.clear();
    updated.instructions = "Use the revised instructions".into();
    updated.model.as_mut().unwrap().effort = Some(Effort::Low);
    let updated = save(&client, updated, team.revision).await.unwrap();
    assert_eq!(client.execute(request).await.unwrap(), first);
    let second = start(&client, draft.clone()).await;
    assert_eq!(second.roles.team, Some(updated.reference()));
    assert_eq!(second.roles.instructions, updated.instructions);
    assert!(second.roles.profiles.is_empty());
    assert_eq!(second.config.effort, Effort::Low);
    draft.team = Some(team.reference().into());
    assert_eq!(
        client
            .execute(client.prepare(Command::StartSession(draft.clone())))
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );

    let first_session = read(&client, first_turn.session).await;
    let selected = revise(
        &client,
        Command::SetSessionExecution {
            session: first_session.id,
            expected_revision: first_session.revision,
            execution: Execution::Team,
            team: Some(team::Selection {
                id: updated.id,
                revision: None,
            }),
            models: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(selected.revision, first_session.revision + 1);
    assert_eq!(selected.roles.team, Some(updated.reference()));
    assert_eq!(selected.config.effort, Effort::Low);
    assert_eq!(selected.association, first_session.association);
    let model = revise(
        &client,
        Command::SetSessionExecution {
            session: selected.id,
            expected_revision: selected.revision,
            execution: Execution::Model,
            team: None,
            models: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        model.roles,
        role::Snapshot {
            execution: Execution::Model,
            ..Default::default()
        }
    );
    assert_eq!(queue(&client, &model).await.roles, model.roles);
    assert_eq!(model.config, selected.config);
    draft.execution = Execution::Model;
    assert_eq!(
        client
            .execute(client.prepare(Command::StartSession(draft)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn pages_and_restores_associated_sessions() {
    let (directory, node, controller, client) = fixture().await;
    let provider = provider(&client, None).await;
    put(&client, profile("available"), 0).await.unwrap();
    let session = create(&client, directory.path(), &provider).await;
    let draft = draft(&session);
    let association = draft.association.clone().unwrap();
    let first = start(&client, draft.clone()).await;
    let second = start(&client, draft.clone()).await;
    assert_eq!(first.roles.execution, Execution::Model);
    assert!(first.roles.profiles.is_empty());
    let command = |before, limit| Command::ListAssociatedSessions {
        association: association.clone(),
        before,
        limit,
    };
    let result = client
        .dispatch(client.prepare(command(None, 1)))
        .await
        .unwrap();
    assert!(!result.receipt.durable);
    let Output::AssociatedSessions(page) = result.completion.await.unwrap().unwrap() else {
        panic!("associated sessions expected")
    };
    assert_eq!(page.sessions.len(), 1);
    assert_eq!(page.sessions[0].id, second.session);
    assert_eq!(page.sessions[0].association.as_ref(), Some(&association));
    assert_eq!(page.next_before, Some(second.session));
    let Output::AssociatedSessions(next) = execute(&client, command(page.next_before, 1)).await
    else {
        panic!("associated sessions expected")
    };
    assert_eq!(next.sessions[0].id, first.session);
    assert_eq!(next.next_before, None);
    for (command, code) in [
        (command(None, 0), ErrorCode::InvalidRequest),
        (command(None, 101), ErrorCode::InvalidRequest),
        (command(Some(session.id), 1), ErrorCode::WrongTarget),
    ] {
        assert_eq!(
            client
                .execute(client.prepare(command))
                .await
                .unwrap_err()
                .code,
            code
        );
    }
    let renamed = revise(
        &client,
        Command::RenameSession {
            session: next.sessions[0].id,
            expected_revision: next.sessions[0].revision,
            title: "Associated task".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(renamed.association.as_ref(), Some(&association));
    node.shutdown().await.unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let client = Client::new(controller.handle().remote(node.link().address()));
    let Output::AssociatedSessions(restored) = execute(&client, command(None, 100)).await else {
        panic!("associated sessions expected")
    };
    assert_eq!(
        restored
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        [second.session, first.session]
    );
    assert!(
        restored
            .sessions
            .iter()
            .all(|session| session.association.as_ref() == Some(&association))
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
