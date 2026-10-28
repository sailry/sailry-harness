use super::*;
use crate::store::agent::tests::fixture::Fixture;
use serde_json::json;

async fn request(fixture: &mut Fixture, worktree: Option<WorktreeId>) -> (TurnId, ToolIdentity) {
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    let request = ToolIdentity {
        name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        id: Some("child".into()),
        arguments: json!({"role":null,"task":"Inspect", "worktree":worktree}),
    };
    let mut event = AdkEvent::new("delegation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.id.clone(),
            name: request.name.clone(),
            args: request.arguments.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    (turn, request)
}

async fn tree(fixture: &Fixture, project: Option<ProjectId>, path: &std::path::Path) -> WorktreeId {
    let id = WorktreeId::new();
    crate::store::worktrees::register(
        &fixture.database.connection,
        &Worktree {
            id,
            project,
            path: path.canonicalize().unwrap().to_str().unwrap().into(),
            main: false,
        },
    )
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn rejects_foreign_or_missing_targets() {
    for foreign in [false, true] {
        let mut fixture = Fixture::new().await;
        crate::store::agent::tests::fixture::package(
            &fixture.database,
            fixture._root.path(),
            "delegation",
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let target = if foreign {
            tree(&fixture, None, dir.path()).await
        } else {
            WorktreeId::new()
        };
        let (turn, request) = request(&mut fixture, Some(target)).await;
        assert!(
            admit(
                &mut fixture.database,
                turn,
                &request,
                request.arguments.clone(),
                None,
                &fixture.events
            )
            .await
            .is_err()
        );
        let count: i64 = (async {
            let mut statement = fixture
                .database
                .connection
                .prepare("SELECT COUNT(*) FROM agent_delegations")
                .await?;
            let row = statement.query_row(()).await?;
            let mapped: turso::Result<_> = row.get(0);
            mapped
        })
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}

#[tokio::test]
async fn inherits_frozen_location() {
    let mut fixture = Fixture::new().await;
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    )
    .await;
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session)
            .await
            .unwrap();
    let original = session.worktree;
    let dir = tempfile::tempdir().unwrap();
    let target = tree(&fixture, session.project, dir.path()).await;
    let (turn, request) = request(&mut fixture, None).await;
    session.worktree = target;
    crate::store::commands::revise_session(&fixture.database.connection, session)
        .await
        .unwrap();
    let child = admit(
        &mut fixture.database,
        turn,
        &request,
        request.arguments.clone(),
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(child.worktree, original);
}

#[tokio::test]
async fn rejects_rebased_input_and_removal() {
    let mut fixture = Fixture::new().await;
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    )
    .await;
    let session = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = tree(&fixture, session.project, dir.path()).await;
    let (turn, request) = request(&mut fixture, Some(target)).await;
    assert!(
        admit(
            &mut fixture.database,
            turn,
            &request,
            request.arguments.clone(),
            Some("Selected input".into()),
            &fixture.events
        )
        .await
        .is_err()
    );
    let removing = Request::new(
        fixture.database.node,
        Command::RemoveWorktree {
            worktree: target,
            expected_head: "a".repeat(40),
            expected_branch: "test".into(),
        },
    );
    let (reply, _response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, removing, reply, &fixture.events)
            .await
            .is_some()
    );
    assert_eq!(
        admit(
            &mut fixture.database,
            turn,
            &request,
            request.arguments.clone(),
            None,
            &fixture.events
        )
        .await
        .err()
        .unwrap()
        .code,
        ErrorCode::Busy
    );
}
