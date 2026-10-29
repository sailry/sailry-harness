use super::super::rules as registry;
use super::*;

fn deadline() -> std::time::Instant {
    std::time::Instant::now() + super::super::TIMEOUT
}

async fn binding(fixture: &Fixture) -> Binding {
    let owner = crate::store::agent::plugins::read(&fixture.database.connection, fixture.turn)
        .await
        .unwrap()
        .into_iter()
        .find(|owner| owner.name == "files")
        .unwrap();
    Binding {
        owner,
        tool: fixture.request.name.clone(),
        operation: Some(sailry_protocol::tool::Operation::WriteFile),
    }
}

async fn begin_owned(fixture: &mut Fixture, binding: &Binding) -> Approval {
    begin(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(binding),
        &fixture.events,
    )
    .await
    .unwrap()
}

async fn close_before_claim(
    fixture: &mut Fixture,
    request: Request,
    response: oneshot::Receiver<Result<Decision, Fault>>,
) -> Result<Output, Fault> {
    let (gate, entered, release) = super::super::fixture::Commit::new(false, false);
    *fixture.database.approval_commit.lock().unwrap() = Some(gate);
    let (reply, result) = oneshot::channel();
    let node = fixture.database.node;
    let execution = async {
        assert!(
            fixture
                .database
                .dispatch(node, request, reply, &fixture.events)
                .await
                .is_none()
        );
        result.await.unwrap().unwrap().completion.await.unwrap()
    };
    let closing = async move {
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .expect("approval did not reach its commit claim");
        drop(response);
        release.send(()).unwrap();
    };
    let (result, ()) = tokio::join!(execution, closing);
    result
}

#[tokio::test]
async fn rolls_back_rules_when_waiter_closes() {
    let mut fixture = Fixture::new().await;
    let owner = binding(&fixture).await;
    let approval = begin_owned(&mut fixture, &owner).await;
    let (reply, response) = oneshot::channel();
    fixture.database.approvals.lock().unwrap().insert(
        approval.id,
        (fixture.turn, RequestId::new(), deadline(), Some(reply)),
    );
    let node = fixture.database.node;
    let request = Request::new(
        node,
        Command::ResolveApproval {
            session: fixture.session,
            approval: approval.id,
            decision: Decision::ApproveSession,
        },
    );
    let error = close_before_claim(&mut fixture, request, response)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_approval_rules")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    prune(&mut fixture.database, &fixture.events).await.unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
}

#[tokio::test]
async fn rejects_closed_receipts() {
    let mut fixture = Fixture::new().await;
    let owner = binding(&fixture).await;
    let approval = begin_owned(&mut fixture, &owner).await;
    let (reply, response) = oneshot::channel();
    fixture.database.approvals.lock().unwrap().insert(
        approval.id,
        (fixture.turn, RequestId::new(), deadline(), Some(reply)),
    );
    let node = fixture.database.node;
    let request = Request::new(
        node,
        Command::ResolveApproval {
            session: fixture.session,
            approval: approval.id,
            decision: Decision::ApproveSession,
        },
    );
    let error = close_before_claim(&mut fixture, request.clone(), response)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    let stored: String = fixture
        .database
        .connection
        .prepare("SELECT result FROM requests WHERE caller=?1 AND id=?2")
        .await
        .unwrap()
        .query_row(params![&node.0[..], request.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Result<Output, Fault>>(&stored)
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(
        fixture
            .database
            .approvals
            .lock()
            .unwrap()
            .contains_key(&approval.id)
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_approval_rules")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    prune(&mut fixture.database, &fixture.events).await.unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
}

#[tokio::test]
async fn skips_closed_session_waiters() {
    let mut fixture = Fixture::new().await;
    let binding = binding(&fixture).await;
    let first = begin_owned(&mut fixture, &binding).await;
    append_call(&mut fixture, "second-write").await;
    let second = begin_owned(&mut fixture, &binding).await;
    let (reply, mut first_response) = oneshot::channel();
    fixture.database.approvals.lock().unwrap().insert(
        first.id,
        (fixture.turn, RequestId::new(), deadline(), Some(reply)),
    );
    let (reply, second_response) = oneshot::channel();
    let owner = RequestId::new();
    fixture
        .database
        .approvals
        .lock()
        .unwrap()
        .insert(second.id, (fixture.turn, owner, deadline(), Some(reply)));
    let request = Request::new(
        fixture.database.node,
        Command::ResolveApproval {
            session: fixture.session,
            approval: first.id,
            decision: Decision::ApproveSession,
        },
    );
    let output = close_before_claim(&mut fixture, request, second_response)
        .await
        .unwrap();
    assert!(
        matches!(output, Output::Approval(approval) if approval.id == first.id && approval.state == ApprovalState::Approved)
    );
    assert_eq!(
        first_response.try_recv().unwrap().unwrap(),
        Decision::Approve
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_approval_rules")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        1
    );
    let pending = get(&fixture.database.connection, second.id).await.unwrap();
    assert_eq!(pending.state, ApprovalState::Pending);
    assert_eq!(pending.source, ApprovalSource::User);
    assert!(
        fixture
            .database
            .approvals
            .lock()
            .unwrap()
            .contains_key(&second.id)
    );
    cancel(
        &mut fixture.database,
        fixture.turn,
        "second-write",
        owner,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, second.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
    assert!(
        !fixture
            .database
            .approvals
            .lock()
            .unwrap()
            .contains_key(&second.id)
    );
}

#[tokio::test]
async fn cleanup_matches_the_waiter_owner() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    let (reply, response) = oneshot::channel();
    let owner = RequestId::new();
    fixture
        .database
        .approvals
        .lock()
        .unwrap()
        .insert(approval.id, (fixture.turn, owner, deadline(), Some(reply)));
    let call = call_id(&fixture.request).unwrap().to_owned();
    cancel(
        &mut fixture.database,
        fixture.turn,
        &call,
        RequestId::new(),
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    drop(response);
    cancel(
        &mut fixture.database,
        fixture.turn,
        &call,
        owner,
        &fixture.events,
    )
    .await
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
    assert!(
        !fixture
            .database
            .approvals
            .lock()
            .unwrap()
            .contains_key(&approval.id)
    );
}

#[tokio::test]
async fn reports_failed_expiry_without_approval() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    let (reply, response) = oneshot::channel();
    let owner = RequestId::new();
    fixture
        .database
        .approvals
        .lock()
        .unwrap()
        .insert(approval.id, (fixture.turn, owner, deadline(), Some(reply)));
    let (gate, entered, release) = super::super::fixture::Commit::new(true, true);
    *fixture.database.approval_commit.lock().unwrap() = Some(gate);
    let call = call_id(&fixture.request).unwrap().to_owned();
    let controls = crate::agent::Controls::default();
    let execution = super::super::super::execute(
        &mut fixture.database,
        Operation::CancelApproval {
            turn: fixture.turn,
            call,
            owner,
        },
        &fixture.events,
        &controls,
    );
    let release = async move {
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .expect("approval cancellation did not reach its commit");
        release.send(()).unwrap();
    };
    tokio::join!(execution, release);
    assert_eq!(
        response.await.unwrap().unwrap_err().code,
        ErrorCode::Internal
    );
    assert_eq!(
        check_waiter(&fixture.database.approvals, approval.id)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    assert!(
        fixture.database.approvals.lock().unwrap()[&approval.id]
            .3
            .is_none()
    );
    assert_eq!(
        fixture
            .database
            .connection
            .prepare("SELECT count(*) FROM agent_approval_rules")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        0
    );
    // The error is honest: no cancelled state was committed. A later ordinary
    // cleanup can settle the closed owner without reviving its approval.
    prune(&mut fixture.database, &fixture.events).await.unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
    assert!(
        !fixture
            .database
            .approvals
            .lock()
            .unwrap()
            .contains_key(&approval.id)
    );
}

async fn append_call(fixture: &mut Fixture, id: &str) {
    fixture.request.id = Some(id.into());
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::ToolRequest(tool_request(
            fixture.request.id.as_deref().expect("call identity"),
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
}

#[tokio::test]
async fn preserves_user_source_and_matches_owner() {
    let mut fixture = Fixture::new().await;
    let binding = binding(&fixture).await;
    let approval = begin_owned(&mut fixture, &binding).await;
    assert!(approval.can_remember);
    let resolved = resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::ApproveSession,
    )
    .await
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
        .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let first = authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
        .await
        .unwrap();
    append_call(&mut fixture, "next-call").await;
    let automatic = begin_owned(&mut fixture, &binding).await;
    assert_eq!(automatic.source, ApprovalSource::SessionRule);
    assert_eq!(automatic.choice, Some(ApprovalChoice::Session));
    assert_eq!(automatic.state, ApprovalState::Approved);
    assert_ne!(automatic.id, approval.id);
    assert_ne!(
        authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
            .await
            .unwrap(),
        first
    );
    assert!(
        authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn applies_session_choice_to_pending_matches() {
    let mut fixture = Fixture::new().await;
    let binding = binding(&fixture).await;
    let first = begin_owned(&mut fixture, &binding).await;
    append_call(&mut fixture, "second-write").await;
    let second = begin_owned(&mut fixture, &binding).await;
    append_call(&mut fixture, "third-write").await;
    let third = begin_owned(&mut fixture, &binding).await;
    assert_eq!(second.state, ApprovalState::Pending);
    assert_eq!(third.state, ApprovalState::Pending);
    command(
        &mut fixture.database,
        &fixture.events,
        Command::ResolveApproval {
            session: fixture.session,
            approval: first.id,
            decision: Decision::ApproveSession,
        },
    )
    .await;
    for id in [second.id, third.id] {
        let approval = get(&fixture.database.connection, id).await.unwrap();
        assert_eq!(approval.state, ApprovalState::Approved);
        assert_eq!(approval.source, ApprovalSource::SessionRule);
        assert_eq!(approval.choice, Some(ApprovalChoice::Session));
    }
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM events ORDER BY cursor DESC LIMIT 1")
        .await
        .unwrap()
        .query_row(())
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    let Event::ConversationChanged { session, activity } = serde_json::from_slice(&body).unwrap()
    else {
        panic!("approval activity expected")
    };
    assert_eq!(session, fixture.session);
    assert_eq!(activity.waiting, None);
    assert_eq!(
        super::super::super::activity::read(&fixture.database.connection, session)
            .await
            .unwrap()
            .waiting,
        None
    );
}

#[tokio::test]
async fn releases_matching_waiters_and_updates() {
    let mut fixture = Fixture::new().await;
    let binding = binding(&fixture).await;
    let first = begin_owned(&mut fixture, &binding).await;
    append_call(&mut fixture, "second-write").await;
    let second = begin_owned(&mut fixture, &binding).await;
    append_call(&mut fixture, "third-write").await;
    let third = begin_owned(&mut fixture, &binding).await;
    fixture.request.name = crate::plugins::tools::alias("files", "create_directory");
    fixture.request.arguments = json!({"path":"unrelated"});
    append_call(&mut fixture, "other-tool").await;
    let other_binding = Binding {
        owner: binding.owner.clone(),
        tool: fixture.request.name.clone(),
        operation: Some(sailry_protocol::tool::Operation::CreateDirectory),
    };
    let unrelated = begin_owned(&mut fixture, &other_binding).await;
    fixture.request.name = "install_skill".into();
    append_call(&mut fixture, "unowned-tool").await;
    let unowned = fixture.begin().await;
    let mut matching = Vec::new();
    let mut unmatched = Vec::new();
    for approval in [&first, &second, &third, &unrelated, &unowned] {
        let (reply, response) = oneshot::channel();
        fixture.database.approvals.lock().unwrap().insert(
            approval.id,
            (approval.turn, RequestId::new(), deadline(), Some(reply)),
        );
        if [first.id, second.id, third.id].contains(&approval.id) {
            matching.push(response);
        } else {
            unmatched.push(response);
        }
    }
    let mut subscription = fixture
        .database
        .feeds
        .subscribe(
            fixture.database.node,
            history::pages::read(&fixture.database.connection, fixture.session, None, 100)
                .await
                .unwrap(),
            Default::default(),
            Default::default(),
            sailry_link::CancellationToken::new(),
        )
        .unwrap();
    assert!(matches!(
        subscription.next().await.unwrap(),
        Update::ConversationSnapshot(_)
    ));
    let Output::Approval(resolved) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ResolveApproval {
            session: fixture.session,
            approval: first.id,
            decision: Decision::ApproveSession,
        },
    )
    .await
    else {
        panic!("approval expected")
    };
    assert_eq!(resolved.source, ApprovalSource::User);
    for response in &mut matching {
        assert_eq!(response.try_recv().unwrap().unwrap(), Decision::Approve);
    }
    for response in &mut unmatched {
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }
    for approval in [&unrelated, &unowned] {
        assert!(
            fixture
                .database
                .approvals
                .lock()
                .unwrap()
                .contains_key(&approval.id)
        );
        assert_eq!(
            get(&fixture.database.connection, approval.id)
                .await
                .unwrap()
                .state,
            ApprovalState::Pending
        );
    }
    let updates = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let mut ids = std::collections::BTreeSet::new();
        for _ in 0..3 {
            let Update::ConversationFrame(frame) = subscription.next().await.unwrap() else {
                panic!("conversation frame expected")
            };
            let Change::Approval(approval) = frame.change else {
                panic!("approval update expected")
            };
            assert_eq!(approval.state, ApprovalState::Approved);
            ids.insert(approval.id);
        }
        ids
    })
    .await
    .unwrap();
    assert_eq!(updates, [first.id, second.id, third.id].into());
}

#[tokio::test]
async fn requires_every_captured_dimension() {
    let mut fixture = Fixture::new().await;
    let binding = binding(&fixture).await;
    let approval = begin_owned(&mut fixture, &binding).await;
    resolve(
        &fixture.database.connection,
        fixture.session,
        approval.id,
        Decision::ApproveSession,
    )
    .await
    .unwrap();
    let db = &fixture.database.connection;
    let run = active(db, fixture.turn).await.unwrap();
    assert!(
        registry::matching(db, &run, &binding, &fixture.request)
            .await
            .unwrap()
            .is_some()
    );
    let mut other = run.clone();
    other.session = SessionId::new();
    assert!(
        registry::matching(db, &other, &binding, &fixture.request)
            .await
            .unwrap()
            .is_none()
    );
    other = run.clone();
    other.worktree = WorktreeId::new();
    assert!(
        registry::matching(db, &other, &binding, &fixture.request)
            .await
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
                .await
                .unwrap()
                .is_none(),
            "{other:?}"
        );
    }
}

#[tokio::test]
async fn rejects_foreign_owner_and_tool() {
    let mut fixture = Fixture::new().await;
    let mut binding = binding(&fixture).await;
    binding.owner.settings_revision += 1;
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
    binding = self::binding(&fixture).await;
    binding.tool = "other".into();
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
    assert!(
        list(&fixture.database.connection, fixture.session, &[])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn keeps_unowned_tools_once_only() {
    let mut fixture = Fixture::new().await;
    fixture.request.name = "install_skill".into();
    append_call(&mut fixture, "core-call").await;
    let approval = fixture.begin().await;
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
            .await
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
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
    .await
    .unwrap();
    authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn preserves_base_permission_precedence() {
    for (permission, mode, source) in [
        (
            Permission::Project,
            WorkMode::Code,
            Some(ApprovalSource::Project),
        ),
        (Permission::Full, WorkMode::Code, Some(ApprovalSource::Full)),
        (Permission::Ask, WorkMode::Plan, None),
    ] {
        let mut fixture = Fixture::with_mode(permission, mode).await;
        let binding = binding(&fixture).await;
        let run = runs::get(&fixture.database.connection, fixture.turn)
            .await
            .unwrap();
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
            startup: None,
            ssh: None,
            can_remember: true,
            access: None,
        };
        fixture.database.connection.execute("INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'earlier',?3,?4,?5,1)",params![remembered.id.to_string(),fixture.turn.to_string(),encode(&remembered).unwrap(),encode(&Some(&binding)).unwrap(),RequestId::new().to_string()]).await.unwrap();
        registry::insert(
            &fixture.database.connection,
            &run,
            &binding,
            &ApprovalChoice::Session,
            remembered.id,
        )
        .await
        .unwrap();
        let result = begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&binding),
            &fixture.events,
        )
        .await;
        if let Some(source) = source {
            let approval = result.unwrap();
            assert_eq!(approval.source, source);
            assert!(!approval.can_remember);
        } else {
            assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
        }
    }
}

#[tokio::test]
async fn validates_prepared_prefix_before_claim() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    fixture.approve(approval.id).await;
    let mut approved = get(&fixture.database.connection, approval.id)
        .await
        .unwrap();
    approved.choice = Some(ApprovalChoice::Prefix("cargo test".into()));
    approved.access = Some(sailry_protocol::process::Access::Workspace);
    put(&fixture.database.connection, &approved).await.unwrap();
    let mut prepared = Command::RunCommand {
        turn: fixture.turn,
        command: "cargo build".into(),
        access: sailry_protocol::process::Access::Workspace,
        description: None,
        cwd: ".".into(),
        timeout_ms: 1000,
        pty: false,
        background: false,
        attachments: vec![],
    };
    assert_eq!(
        authorize(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            Some(&prepared)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let claimed: bool = fixture
        .database
        .connection
        .prepare("SELECT claimed FROM agent_approvals WHERE id=?1")
        .await
        .unwrap()
        .query_row([approval.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(!claimed);
    let Command::RunCommand { command, .. } = &mut prepared else {
        unreachable!()
    };
    *command = "cargo test --lib".into();
    authorize(
        &mut fixture.database,
        fixture.turn,
        &fixture.request,
        Some(&prepared),
    )
    .await
    .unwrap();
    let claimed: bool = fixture
        .database
        .connection
        .prepare("SELECT claimed FROM agent_approvals WHERE id=?1")
        .await
        .unwrap()
        .query_row([approval.id.to_string()])
        .await
        .and_then(|row| row.get(0))
        .unwrap();
    assert!(claimed);
}

#[tokio::test]
async fn compares_closed_choices_without_history() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    fixture.approve(approval.id).await;
    let mut approved = get(&fixture.database.connection, approval.id)
        .await
        .unwrap();
    approved.choice = Some(ApprovalChoice::Prefix("cargo test".into()));
    approved.entry = "unavailable-entry".into();
    put(&fixture.database.connection, &approved).await.unwrap();
    let db = &fixture.database.connection;
    assert_eq!(
        resolve(
            db,
            fixture.session,
            approval.id,
            Decision::ApprovePrefix(" cargo\ttest ".into())
        )
        .await
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
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    assert_eq!(get(db, approval.id).await.unwrap(), approved);
    assert_eq!(
        db.prepare("SELECT count(*) FROM agent_approval_rules")
            .await
            .unwrap()
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .unwrap(),
        0
    );
}
