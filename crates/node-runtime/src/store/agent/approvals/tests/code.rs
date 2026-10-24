use super::*;
use sailry_protocol::{WorkMode, code::Startup};

async fn fixture(permission: Permission, mode: WorkMode) -> (Fixture, Binding, Startup) {
    let mut fixture = Fixture::with_catalog(permission, mode, true).await;
    let owner = super::super::super::plugins::read(&fixture.database.connection, fixture.turn)
        .await
        .unwrap()
        .into_iter()
        .find(|owner| owner.name == "code")
        .unwrap();
    fixture.request = ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: crate::plugins::tools::alias("code", "code_hover"),
        function_call_id: Some("code-call".into()),
        args: json!({"path":"main.rs","line":1,"symbol":"name"}),
    };
    let mut event = AdkEvent::new("code");
    event.author = "assistant".into();
    event.set_content(adk_core::Content {
        role: "model".into(),
        parts: vec![adk_core::Part::FunctionCall {
            id: fixture.request.function_call_id.clone(),
            name: fixture.request.tool_name.clone(),
            args: fixture.request.args.clone(),
            thought_signature: None,
        }],
    });
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let run = super::super::super::calls::active(&fixture.database.connection, fixture.turn)
        .await
        .unwrap();
    let binding = Binding {
        owner,
        tool: fixture.request.tool_name.clone(),
        operation: Some(sailry_protocol::tool::Operation::CodeHover),
    };
    let startup = Startup {
        worktree: run.worktree,
        root: String::new(),
        server: "rust-analyzer".into(),
        executable: "/fixture/rust-analyzer".into(),
        revision: "a".repeat(64),
    };
    (fixture, binding, startup)
}

#[tokio::test]
async fn requires_exact_claim_for_plan_lifecycle() {
    for mode in [WorkMode::Plan, WorkMode::Code] {
        let (mut fixture, binding, startup) = fixture(Permission::Ask, mode).await;
        let approval = begin_with(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            Some(&startup),
            &fixture.events,
        )
        .await
        .unwrap();
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!approval.can_remember);
        assert!(approval.prefix.is_none());
        assert_eq!(
            resolve(
                &fixture.database.connection,
                fixture.session,
                approval.id,
                Decision::ApproveSession
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        fixture.approve(approval.id).await;
        let mut forged = startup.clone();
        forged.revision = "b".repeat(64);
        assert_eq!(
            authorize(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&Command::StartCodeServer { startup: forged })
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::PermissionDenied
        );
        assert!(
            !crate::store::code::approved(&fixture.database.connection, &startup)
                .await
                .unwrap()
        );
        let id = authorize(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&Command::StartCodeServer {
                startup: startup.clone(),
            }),
        )
        .await
        .unwrap();
        assert!(
            crate::store::code::approved(&fixture.database.connection, &startup)
                .await
                .unwrap()
        );
        let context = plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: plugin::desktop::Surface::Workspace,
            package: binding.owner.clone(),
            worktree: Some(startup.worktree),
            session: Some(fixture.session),
        };
        let mut request = Request::new(
            fixture.database.node,
            Command::StartCodeServer {
                startup: startup.clone(),
            },
        )
        .with_plugin(context);
        request.id = id;
        assert!(
            crate::store::code::prepare(&fixture.database, &request)
                .await
                .is_ok()
        );
        for change in 0..5 {
            let mut forged = request.clone();
            let context = forged.plugin.as_mut().unwrap();
            match change {
                0 => forged.id = RequestId::new(),
                1 => context.package.digest = "c".repeat(64),
                2 => context.session = Some(SessionId::new()),
                3 => context.turn = Some(TurnId::new()),
                _ => context.worktree = Some(WorktreeId::new()),
            };
            assert_eq!(
                crate::store::code::prepare(&fixture.database, &forged)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let database = Database::open(&fixture.path, fixture.database.node, None)
            .await
            .unwrap();
        assert!(
            crate::store::code::approved(&database.connection, &startup)
                .await
                .unwrap()
        );
        let mut changed = startup;
        changed.revision = "d".repeat(64);
        assert!(
            !crate::store::code::approved(&database.connection, &changed)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn permits_project_and_full_with_plan_limits() {
    for (permission, source) in [
        (Permission::Project, ApprovalSource::Project),
        (Permission::Full, ApprovalSource::Full),
    ] {
        for mode in [WorkMode::Plan, WorkMode::Code] {
            let (mut fixture, binding, startup) = fixture(permission, mode).await;
            let approval = begin_with(
                &mut fixture.database,
                fixture.turn,
                &fixture.request,
                Some(&binding),
                Some(&startup),
                &fixture.events,
            )
            .await
            .unwrap();
            assert_eq!(approval.state, ApprovalState::Approved);
            assert_eq!(approval.source, source);
            let mut wrong = fixture.request.clone();
            wrong.args["path"] = json!("elsewhere.rs");
            assert_eq!(
                authorize(
                    &mut fixture.database,
                    fixture.turn,
                    &wrong,
                    Some(&Command::StartCodeServer {
                        startup: startup.clone()
                    })
                )
                .await
                .unwrap_err()
                .code,
                ErrorCode::PermissionDenied
            );
            if mode == WorkMode::Plan {
                assert_eq!(
                    super::super::super::permissions::source(
                        &fixture.database.connection,
                        fixture.turn,
                        &ToolConfirmationRequest {
                            acting: None,
                            origin: None,
                            tool_name: crate::plugins::tools::alias("files", "write_file"),
                            function_call_id: Some("write".into()),
                            args: json!({})
                        }
                    )
                    .await
                    .unwrap_err()
                    .code,
                    ErrorCode::PermissionDenied
                );
            }
        }
    }
}

#[tokio::test]
async fn reuses_only_exact_workspace_server_grants() {
    let (mut fixture, binding, startup) = fixture(Permission::Ask, WorkMode::Plan).await;
    crate::store::code::grant(&fixture.database.connection, &startup)
        .await
        .unwrap();
    let approval = begin_with(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(&binding),
        Some(&startup),
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(approval.source, ApprovalSource::CodeRule);
    assert_eq!(approval.state, ApprovalState::Approved);
    let mut changed = startup.clone();
    changed.worktree = WorktreeId::new();
    assert_eq!(
        super::super::super::permissions::code_source(
            &fixture.database.connection,
            fixture.turn,
            &fixture.request,
            &changed
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    for change in 0..4 {
        let mut changed = startup.clone();
        match change {
            0 => changed.server = "other".into(),
            1 => changed.root = "other".into(),
            2 => changed.revision = "b".repeat(64),
            _ => changed.executable = "/other/tool".into(),
        };
        assert_eq!(
            super::super::super::permissions::code_source(
                &fixture.database.connection,
                fixture.turn,
                &fixture.request,
                &changed
            )
            .await
            .unwrap(),
            ApprovalSource::User
        );
    }
}
