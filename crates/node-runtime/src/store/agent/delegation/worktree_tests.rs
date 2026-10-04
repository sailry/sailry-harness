use super::*;
use crate::store::agent::tests::fixture::Fixture;
use serde_json::json;

fn request(
    fixture: &mut Fixture,
    worktree: Option<WorktreeId>,
) -> (TurnId, ToolConfirmationRequest) {
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    let request = ToolConfirmationRequest {
        tool_name: crate::plugins::tools::alias("delegation", "spawn_agent"),
        function_call_id: Some("child".into()),
        args: json!({"role":null,"task":"Inspect", "worktree":worktree}),
    };
    let mut event = AdkEvent::new("delegation");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: request.function_call_id.clone(),
            name: request.tool_name.clone(),
            args: request.args.clone(),
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
    .unwrap();
    (turn, request)
}

fn tree(fixture: &Fixture, project: Option<ProjectId>, path: &std::path::Path) -> WorktreeId {
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
    .unwrap();
    id
}

#[test]
fn rejects_foreign_or_missing_targets() {
    for foreign in [false, true] {
        let mut fixture = Fixture::new();
        crate::store::agent::tests::fixture::package(
            &fixture.database,
            fixture._root.path(),
            "delegation",
        );
        let dir = tempfile::tempdir().unwrap();
        let target = if foreign {
            tree(&fixture, None, dir.path())
        } else {
            WorktreeId::new()
        };
        let (turn, request) = request(&mut fixture, Some(target));
        assert!(
            admit(
                &mut fixture.database,
                turn,
                &request,
                request.args.clone(),
                None,
                &fixture.events
            )
            .is_err()
        );
        let count: i64 = fixture
            .database
            .connection
            .query_row("SELECT COUNT(*) FROM session_delegations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[test]
fn inherits_frozen_location() {
    let mut fixture = Fixture::new();
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    );
    let mut session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let original = session.worktree;
    let dir = tempfile::tempdir().unwrap();
    let target = tree(&fixture, session.project, dir.path());
    let (turn, request) = request(&mut fixture, None);
    session.worktree = target;
    crate::store::commands::revise_session(&fixture.database.connection, session).unwrap();
    let child = admit(
        &mut fixture.database,
        turn,
        &request,
        request.args.clone(),
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(child.worktree, original);
}

#[test]
fn rejects_rebased_input_and_removal() {
    let mut fixture = Fixture::new();
    crate::store::agent::tests::fixture::package(
        &fixture.database,
        fixture._root.path(),
        "delegation",
    );
    let session =
        crate::store::commands::session(&fixture.database.connection, fixture.session).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = tree(&fixture, session.project, dir.path());
    let (turn, request) = request(&mut fixture, Some(target));
    assert!(
        admit(
            &mut fixture.database,
            turn,
            &request,
            request.args.clone(),
            Some("Selected input".into()),
            &fixture.events
        )
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
            .is_some()
    );
    assert_eq!(
        admit(
            &mut fixture.database,
            turn,
            &request,
            request.args.clone(),
            None,
            &fixture.events
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::Busy
    );
}
