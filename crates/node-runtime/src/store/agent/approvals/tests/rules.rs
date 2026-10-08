use super::super::rules as registry;
use super::*;

fn binding(fixture: &Fixture) -> Binding {
    let owner = crate::store::agent::plugins::read(&fixture.database.connection, fixture.turn)
        .unwrap()
        .into_iter()
        .find(|owner| owner.name == "files")
        .unwrap();
    Binding {
        owner,
        tool: fixture.request.tool_name.clone(),
        operation: Some(sailry_protocol::tool::Operation::WriteFile),
    }
}

fn begin_owned(fixture: &mut Fixture, binding: &Binding) -> Approval {
    begin(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(binding),
        &fixture.events,
    )
    .unwrap()
}

fn append_call(fixture: &mut Fixture, id: &str) {
    fixture.request.function_call_id = Some(id.into());
    let mut event = AdkEvent::new("fixture");
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
    .unwrap();
}

#[test]
fn preserves_user_source_and_matches_owner() {
    let mut fixture = Fixture::new();
    let binding = binding(&fixture);
    let approval = begin_owned(&mut fixture, &binding);
    assert!(approval.can_remember);
    let resolved = resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::ApproveSession,
    )
    .unwrap();
    assert_eq!(resolved.source, ApprovalSource::User);
    assert_eq!(resolved.choice, Some(ApprovalChoice::Session));
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            approval.id,
            Decision::ApproveSession
        )
        .unwrap(),
        resolved
    );
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            approval.id,
            Decision::Approve
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let first = authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        None,
    )
    .unwrap();
    append_call(&mut fixture, "next-call");
    let automatic = begin_owned(&mut fixture, &binding);
    assert_eq!(automatic.source, ApprovalSource::SessionRule);
    assert_eq!(automatic.choice, Some(ApprovalChoice::Session));
    assert_eq!(automatic.state, ApprovalState::Approved);
    assert_ne!(automatic.id, approval.id);
    assert_ne!(
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            None
        )
        .unwrap(),
        first
    );
    assert!(
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            None
        )
        .is_err()
    );
}

#[test]
fn requires_every_captured_dimension() {
    let mut fixture = Fixture::new();
    let binding = binding(&fixture);
    let approval = begin_owned(&mut fixture, &binding);
    resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::ApproveSession,
    )
    .unwrap();
    let db = &fixture.database.connection;
    let run = active(db, fixture.turn).unwrap();
    assert!(
        registry::matching(db, &run, &binding, &fixture.request)
            .unwrap()
            .is_some()
    );
    let mut other = run.clone();
    other.session = SessionId::new();
    assert!(
        registry::matching(db, &other, &binding, &fixture.request)
            .unwrap()
            .is_none()
    );
    other = run.clone();
    other.worktree = WorktreeId::new();
    assert!(
        registry::matching(db, &other, &binding, &fixture.request)
            .unwrap()
            .is_none()
    );
    let mut owners = Vec::new();
    let mut other = binding.clone();
    other.owner.name.push_str("-other");
    owners.push(other);
    let mut other = binding.clone();
    other.owner.digest.push_str("-other");
    owners.push(other);
    let mut other = binding.clone();
    other.owner.settings_revision += 1;
    owners.push(other);
    let mut other = binding.clone();
    other.tool.push_str("-other");
    owners.push(other);
    let mut other = binding.clone();
    other.operation = Some(sailry_protocol::tool::Operation::EditFile);
    owners.push(other);
    for other in owners {
        assert!(
            registry::matching(db, &run, &other, &fixture.request)
                .unwrap()
                .is_none(),
            "{other:?}"
        );
    }
}

#[test]
fn rejects_foreign_owner_and_tool() {
    let mut fixture = Fixture::new();
    let mut binding = binding(&fixture);
    binding.owner.settings_revision += 1;
    assert_eq!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    binding = self::binding(&fixture);
    binding.tool = "other".into();
    assert_eq!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    assert!(
        list(&fixture.database.connection, fixture.session, &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn keeps_unowned_tools_once_only() {
    let mut fixture = Fixture::new();
    fixture.request.tool_name = "install_skill".into();
    append_call(&mut fixture, "core-call");
    let approval = fixture.begin();
    assert!(!approval.can_remember);
    assert!(approval.prefix.is_none());
    for decision in [
        Decision::ApproveSession,
        Decision::ApprovePrefix("cargo test".into()),
    ] {
        assert_eq!(
            resolve(
                &fixture.database.connection,
                fixture.session,
                approval.id,
                decision
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::Approve,
    )
    .unwrap();
    authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        None,
    )
    .unwrap();
}

#[test]
fn preserves_base_permission_precedence() {
    for (permission, mode, source) in [
        (
            Permission::Project,
            WorkMode::Code,
            Some(ApprovalSource::Project),
        ),
        (Permission::Full, WorkMode::Code, Some(ApprovalSource::Full)),
        (Permission::Ask, WorkMode::Plan, None),
    ] {
        let mut fixture = Fixture::with_mode(permission, mode);
        let binding = binding(&fixture);
        let run = runs::get(&fixture.database.connection, fixture.turn).unwrap();
        // A previous explicit user decision is durable independently of this turn's policy.
        let remembered = Approval {
            id: ApprovalId::new(),
            session: fixture.session,
            turn: fixture.turn,
            entry: "fixture".into(),
            index: 0,
            state: ApprovalState::Approved,
            source: ApprovalSource::User,
            choice: Some(ApprovalChoice::Session),
            reason: None,
            prefix: None,
            can_remember: true,
        };
        fixture.database.connection.execute("INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'earlier',?3,?4,?5,1)",params![remembered.id.to_string(),fixture.turn.to_string(),encode(&remembered).unwrap(),encode(&Some(&binding)).unwrap(),RequestId::new().to_string()]).unwrap();
        registry::insert(
            &fixture.database.connection,
            &run,
            &binding,
            &ApprovalChoice::Session,
            remembered.id,
        )
        .unwrap();
        let result = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        );
        if let Some(source) = source {
            let approval = result.unwrap();
            assert_eq!(approval.source, source);
            assert!(!approval.can_remember);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
        }
    }
}

#[test]
fn validates_prepared_prefix_before_claim() {
    let mut fixture = Fixture::new();
    let approval = fixture.begin();
    fixture.approve(approval.id);
    let mut approved = get(&fixture.database.connection, approval.id).unwrap();
    approved.choice = Some(ApprovalChoice::Prefix("cargo test".into()));
    put(&fixture.database.connection, &approved).unwrap();
    let mut prepared = Command::RunCommand {
        turn: fixture.turn,
        command: "cargo build".into(),
        description: None,
        cwd: ".".into(),
        timeout_ms: 1000,
        background: false,
        attachments: vec![],
    };
    assert_eq!(
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request,
            Some(&prepared)
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let claimed: bool = fixture
        .database
        .connection
        .query_row(
            "SELECT claimed FROM agent_approvals WHERE id=?1",
            [approval.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!claimed);
    let Command::RunCommand { command, .. } = &mut prepared else {
        unreachable!()
    };
    *command = "cargo test --lib".into();
    authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
        Some(&prepared),
    )
    .unwrap();
}

#[test]
fn compares_closed_choices_without_history() {
    let mut fixture = Fixture::new();
    let approval = fixture.begin();
    fixture.approve(approval.id);
    let mut approved = get(&fixture.database.connection, approval.id).unwrap();
    approved.choice = Some(ApprovalChoice::Prefix("cargo test".into()));
    approved.entry = "unavailable-entry".into();
    put(&fixture.database.connection, &approved).unwrap();
    let db = &fixture.database.connection;
    assert_eq!(
        resolve(
            db,
            fixture.session,
            approval.id,
            Decision::ApprovePrefix(" cargo\ttest ".into())
        )
        .unwrap(),
        approved
    );
    for decision in [
        Decision::Approve,
        Decision::ApprovePrefix("cargo test;".into()),
        Decision::DenyWithReason("".into()),
    ] {
        assert_eq!(
            resolve(db, fixture.session, approval.id, decision)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    assert_eq!(get(db, approval.id).unwrap(), approved);
    assert_eq!(
        db.query_row("SELECT count(*) FROM agent_approval_rules", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
}
