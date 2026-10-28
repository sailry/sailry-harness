use super::*;
use sailry_protocol::tool::Operation;

async fn fixture(operation: Operation, mode: WorkMode) -> (Fixture, Binding, Command) {
    let mut fixture = Fixture::with_package(Permission::Ask, mode, Some("delegation")).await;
    let owner = super::super::super::plugins::read(&fixture.database.connection, fixture.turn)
        .await
        .unwrap()
        .into_iter()
        .find(|package| package.name == "delegation")
        .unwrap();
    let role = crate::store::roles::list(&fixture.database.connection)
        .await
        .unwrap()
        .remove(0);
    let (name, command) = match operation {
        Operation::SaveRole => (
            "edit_agent",
            Command::PutRole {
                role: role.clone(),
                expected_revision: role.revision,
            },
        ),
        Operation::RemoveRole => (
            "delete_agent",
            Command::RemoveRole {
                role: role.id,
                expected_revision: role.revision,
            },
        ),
        Operation::ApplyRoles => (
            "apply_agents",
            Command::SetSessionRoles {
                session: fixture.session,
                expected_revision: 1,
                roles: vec![role.reference()],
            },
        ),
        _ => unreachable!(),
    };
    fixture.request = ToolIdentity {
        name: crate::plugins::tools::alias("delegation", name),
        id: Some("role-call".into()),
        arguments: json!({"fixture":"exact canonical arguments"}),
    };
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::ToolRequest(tool_request(
            fixture.request.id.as_deref(),
            &fixture.request.name,
            fixture.request.arguments.clone(),
        ))],
    );
    event.turn = fixture.turn;
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let binding = Binding {
        owner,
        tool: fixture.request.name.clone(),
        operation: Some(operation),
    };
    (fixture, binding, command)
}

#[tokio::test]
async fn requires_exact_claimed_commands() {
    for operation in [
        Operation::SaveRole,
        Operation::RemoveRole,
        Operation::ApplyRoles,
    ] {
        let (mut fixture, binding, command) = fixture(operation, WorkMode::Code).await;
        let run = super::super::super::calls::active(&fixture.database.connection, fixture.turn)
            .await
            .unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: plugin::desktop::Surface::Workspace,
            package: binding.owner.clone(),
            worktree: Some(run.worktree),
            session: Some(fixture.session),
        };
        let mut request = Request::new(fixture.database.node, command.clone()).with_plugin(context);
        assert_eq!(
            crate::store::roles::approvals::check(&fixture.database.connection, &request)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let approval = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .await
        .unwrap();
        fixture.approve(approval.id).await;
        request.id = authorize(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&command),
        )
        .await
        .unwrap();
        assert!(
            crate::store::roles::approvals::check(&fixture.database.connection, &request)
                .await
                .is_ok()
        );
        let mut changed = request.clone();
        match &mut changed.command {
            Command::PutRole { role, .. } => role.instructions.push_str(" changed"),
            Command::RemoveRole {
                expected_revision, ..
            }
            | Command::SetSessionRoles {
                expected_revision, ..
            } => *expected_revision += 1,
            _ => unreachable!(),
        }
        assert_eq!(
            crate::store::roles::approvals::check(&fixture.database.connection, &changed)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        changed = request.clone();
        changed.id = RequestId::new();
        assert_eq!(
            crate::store::roles::approvals::check(&fixture.database.connection, &changed)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        for foreign in ["session", "worktree", "package", "callback"] {
            let mut changed = request.clone();
            let context = changed.plugin.as_mut().unwrap();
            match foreign {
                "session" => context.session = Some(SessionId::new()),
                "worktree" => context.worktree = Some(WorktreeId::new()),
                "package" => context.package.name = "foreign".into(),
                "callback" => context.invocation = Some(RequestId::new()),
                _ => unreachable!(),
            }
            assert_eq!(
                crate::store::roles::approvals::check(&fixture.database.connection, &changed)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
    }
}

#[tokio::test]
async fn planning_denies_mutations() {
    for operation in [
        Operation::SaveRole,
        Operation::RemoveRole,
        Operation::ApplyRoles,
    ] {
        let (mut fixture, binding, _) = fixture(operation, WorkMode::Plan).await;
        assert_eq!(
            begin(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&binding),
                &fixture.events
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
    }
}
