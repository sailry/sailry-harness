use super::*;
use sailry_protocol::tool::Operation;

async fn fixture(mode: WorkMode) -> (Fixture, Binding, Command) {
    let mut fixture = Fixture::with_mode(Permission::Ask, mode).await;
    let owner = super::super::super::plugins::read(&fixture.database.connection, fixture.turn)
        .await
        .unwrap()
        .into_iter()
        .find(|package| package.name == "files")
        .unwrap();
    fixture.request = ToolConfirmationRequest {
        acting: None,
        origin: None,
        tool_name: crate::plugins::tools::alias("files", "uninstall_ocr_model"),
        function_call_id: Some("ocr-call".into()),
        args: json!({"id":"a".repeat(64)}),
    };
    let mut event = AdkEvent::new("ocr-fixture");
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
    let binding = Binding {
        owner,
        tool: fixture.request.tool_name.clone(),
        operation: Some(Operation::RemoveModel),
    };
    (
        fixture,
        binding,
        Command::DeleteLocalModel { id: "a".repeat(64) },
    )
}

#[tokio::test]
async fn removal_consumes_exact_claimed_command() {
    let (mut fixture, binding, command) = fixture(WorkMode::Code).await;
    let run = super::super::super::calls::active(&fixture.database.connection, fixture.turn)
        .await
        .unwrap();
    let mut request =
        Request::new(fixture.database.node, command.clone()).with_plugin(plugin::Context {
            invocation: None,
            turn: Some(fixture.turn),
            surface: Default::default(),
            package: binding.owner.clone(),
            worktree: Some(run.worktree),
            session: Some(run.session),
        });
    assert_eq!(
        crate::store::local_models::approvals::check(&fixture.database.connection, &request)
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
        crate::store::local_models::approvals::check(&fixture.database.connection, &request)
            .await
            .is_ok()
    );
    assert!(
        crate::store::resources::prepare(&fixture.database, &request)
            .await
            .is_ok()
    );
    let mut changed = request.clone();
    changed.command = Command::DeleteLocalModel { id: "b".repeat(64) };
    assert_eq!(
        crate::store::local_models::approvals::check(&fixture.database.connection, &changed)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    changed = request.clone();
    changed.id = RequestId::new();
    assert_eq!(
        crate::store::local_models::approvals::check(&fixture.database.connection, &changed)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    for field in ["package", "session", "worktree", "turn"] {
        changed = request.clone();
        let context = changed.plugin.as_mut().unwrap();
        match field {
            "package" => context.package.name = "foreign".into(),
            "session" => context.session = Some(SessionId::new()),
            "worktree" => context.worktree = Some(WorktreeId::new()),
            "turn" => context.turn = None,
            _ => unreachable!(),
        }
        assert_eq!(
            crate::store::local_models::approvals::check(&fixture.database.connection, &changed)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    let canonical = Prepared::new(&command).unwrap().unwrap();
    assert!(canonical.matches(&command).unwrap());
    assert!(
        !canonical
            .matches(&Command::DeleteLocalModel { id: "c".repeat(64) })
            .unwrap()
    );
}

#[tokio::test]
async fn planning_cannot_claim_removal() {
    let (mut fixture, binding, _) = fixture(WorkMode::Plan).await;
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

#[tokio::test]
async fn preserves_installation_policy() {
    let (fixture, binding, _) = fixture(WorkMode::Code).await;
    let request = Request::new(
        fixture.database.node,
        Command::InstallLocalModel { id: "a".repeat(64) },
    )
    .with_plugin(plugin::Context {
        invocation: None,
        turn: Some(fixture.turn),
        surface: Default::default(),
        package: binding.owner,
        worktree: None,
        session: None,
    });
    assert!(
        crate::store::local_models::approvals::check(&fixture.database.connection, &request)
            .await
            .is_ok()
    );
    let prepared = Prepared::new(&request.command).unwrap().unwrap();
    assert!(prepared.matches(&request.command).unwrap());
    assert!(
        !prepared
            .matches(&Command::InstallLocalModel { id: "b".repeat(64) })
            .unwrap()
    );
}
