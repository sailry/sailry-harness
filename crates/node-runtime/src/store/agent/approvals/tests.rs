use super::super::tests::command;
use super::*;
use serde_json::json;

mod permissions;
mod planning;

struct Fixture {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    turn: TurnId,
    session: SessionId,
    request: ToolConfirmationRequest,
}

impl Fixture {
    fn new() -> Self {
        Self::with_permission(Permission::Ask)
    }

    fn with_permission(permission: Permission) -> Self {
        Self::with_mode(permission, sailry_protocol::WorkMode::Code)
    }

    fn with_mode(permission: Permission, mode: sailry_protocol::WorkMode) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let mut database = Database::open(&path, NodeId([31; 32]), None).unwrap();
        super::super::tests::fixture::files(&database, directory.path());
        let (events, _) = broadcast::channel(64);
        let Output::Project(project) = command(
            &mut database,
            &events,
            Command::RegisterProject {
                name: "Approval fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            },
        ) else {
            panic!("project expected")
        };
        let Output::Session(session) = command(
            &mut database,
            &events,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Low,
                    mode,
                    permission,
                    credential: None,
                }),
            },
        ) else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = command(
            &mut database,
            &events,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "fixture".into(),
            },
        ) else {
            panic!("turn expected")
        };
        assert_eq!(
            runs::claim(&mut database, &events)
                .unwrap()
                .unwrap()
                .turn
                .id,
            turn.id
        );
        let request = ToolConfirmationRequest {
            tool_name: crate::plugins::tools::alias("files", "write_file"),
            function_call_id: Some("call-fixture".into()),
            args: json!({"path": "资料.txt", "text": "中文 🙂", "expected_revision": null}),
        };
        let mut event = AdkEvent::new("fixture");
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
        history::append(&mut database, session.id, turn.id, event, &events).unwrap();
        Self {
            _directory: directory,
            path,
            database,
            events,
            turn: turn.id,
            session: session.id,
            request,
        }
    }

    fn begin(&mut self) -> Approval {
        begin(&mut self.database, self.turn, &self.request, &self.events).unwrap()
    }

    fn approve(&mut self, id: ApprovalId) {
        command(
            &mut self.database,
            &self.events,
            Command::ResolveApproval {
                session: self.session,
                approval: id,
                decision: Decision::Approve,
            },
        );
    }
}

#[test]
fn consumes_exact_arguments_once() {
    let mut fixture = Fixture::new();
    let approval = fixture.begin();
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events
        )
        .is_err()
    );
    assert_eq!(
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    fixture.approve(approval.id);
    let mut changed = fixture.request.clone();
    changed.args["text"] = json!("different content");
    assert_eq!(
        authorize(&mut fixture.database.connection, fixture.turn, &changed)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut changed = fixture.request.clone();
    changed.tool_name = "another_tool".into();
    assert_eq!(
        authorize(&mut fixture.database.connection, fixture.turn, &changed)
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    authorize(
        &mut fixture.database.connection,
        fixture.turn,
        &fixture.request,
    )
    .unwrap();
    assert_eq!(
        authorize(
            &mut fixture.database.connection,
            fixture.turn,
            &fixture.request
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            approval.id,
            Decision::Deny
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
}

#[test]
fn rejects_invalid_decisions() {
    let mut fixture = Fixture::new();
    let mut changed = fixture.request.clone();
    changed.args["path"] = json!("another.txt");
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &changed,
            &fixture.events
        )
        .is_err()
    );
    let approval = fixture.begin();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: fixture.turn },
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
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .unwrap()
            .state,
        ApprovalState::Pending
    );
    runs::finish(
        &mut fixture.database,
        fixture.turn,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
}

#[test]
fn closes_pending_on_restart() {
    let mut fixture = Fixture::new();
    let approval = fixture.begin();
    let node = fixture.database.node;
    drop(fixture.database);
    let database = Database::open(&fixture.path, node, None).unwrap();
    assert_eq!(
        get(&database.connection, approval.id).unwrap().state,
        ApprovalState::Interrupted
    );
    assert_eq!(
        runs::get(&database.connection, fixture.turn)
            .unwrap()
            .status,
        Status::Interrupted
    );
    assert_eq!(
        resolve(
            &database.connection,
            fixture.session,
            approval.id,
            Decision::Approve
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .unwrap()
        .page;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.approvals[0].id, approval.id);
}
