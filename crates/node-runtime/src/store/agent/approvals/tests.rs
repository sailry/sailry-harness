use super::super::tests::command;
use super::*;
use crate::agent::canonical::{BlockKind, Role as MessageRole};
use crate::store::agent::tests::fixture::{message, request as tool_request};
use serde_json::json;

mod access;
mod connections;
mod models;
mod permissions;
mod planning;
mod retired;
mod roles;
mod rules;

struct Fixture {
    path: std::path::PathBuf,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    turn: TurnId,
    session: SessionId,
    request: ToolIdentity,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new() -> sailry_link::Pending<'static, Self> {
        Self::with_permission(Permission::Ask)
    }

    fn with_permission(permission: Permission) -> sailry_link::Pending<'static, Self> {
        Self::with_mode(permission, sailry_protocol::WorkMode::Code)
    }

    fn with_mode(
        permission: Permission,
        mode: sailry_protocol::WorkMode,
    ) -> sailry_link::Pending<'static, Self> {
        Self::with_package(permission, mode, None)
    }

    fn with_package<'a>(
        permission: Permission,
        mode: sailry_protocol::WorkMode,
        package: Option<&'a str>,
    ) -> sailry_link::Pending<'a, Self> {
        Box::pin(async move {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().canonicalize().unwrap().join("node.turso");
            let mut database = Database::open(&path, NodeId([31; 32]), None).await.unwrap();
            super::super::tests::fixture::files(&database, directory.path()).await;
            if let Some(package) = package {
                super::super::tests::fixture::package(&database, directory.path(), package).await;
            }
            let (events, _) = broadcast::channel(64);
            let Output::Project(project) = command(
                &mut database,
                &events,
                Command::RegisterProject {
                    name: "Approval fixture".into(),
                    path: directory.path().to_str().unwrap().into(),
                },
            )
            .await
            else {
                panic!("project expected")
            };
            let Output::Session(session) = command(
                &mut database,
                &events,
                Command::CreateSession {
                    project: Some(project.id),
                    worktree: None,
                    config: Some(SessionConfig {
                        features: Default::default(),
                        assistant: None,
                        resource: None,
                        provider: ProviderId::new(),
                        model: "fixture".into(),
                        effort: Effort::Low,
                        mode,
                        options: Default::default(),
                        permission,
                        credential: None,
                    }),
                },
            )
            .await
            else {
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
            )
            .await
            else {
                panic!("turn expected")
            };
            assert_eq!(
                runs::claim(&mut database, &events)
                    .await
                    .unwrap()
                    .unwrap()
                    .turn
                    .id,
                turn.id
            );
            let request = ToolIdentity {
                name: crate::plugins::tools::alias("files", "write_file"),
                id: Some("call-fixture".into()),
                arguments: json!({"path": "资料.txt", "text": "中文 🙂", "expected_revision": null}),
            };
            let mut event = message(
                "assistant",
                MessageRole::Assistant,
                vec![BlockKind::ToolRequest(tool_request(
                    request.id.as_deref(),
                    &request.name,
                    request.arguments.clone(),
                ))],
            );
            event.turn = turn.id;
            history::append(&mut database, session.id, turn.id, event, &events)
                .await
                .unwrap();
            Self {
                _directory: directory,
                path,
                database,
                events,
                turn: turn.id,
                session: session.id,
                request,
            }
        })
    }

    async fn begin(&mut self) -> Approval {
        begin(
            &mut self.database,
            self.turn,
            &self.request,
            None,
            &self.events,
        )
        .await
        .unwrap()
    }

    async fn approve(&mut self, id: ApprovalId) {
        command(
            &mut self.database,
            &self.events,
            Command::ResolveApproval {
                session: self.session,
                approval: id,
                decision: Decision::Approve,
            },
        )
        .await;
    }
}

#[tokio::test]
async fn consumes_exact_arguments_once() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            None,
            &fixture.events
        )
        .await
        .is_err()
    );
    assert_eq!(
        authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    fixture.approve(approval.id).await;
    let mut changed = fixture.request.clone();
    changed.arguments["text"] = json!("different content");
    assert_eq!(
        authorize(&mut fixture.database, fixture.turn, &changed, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    let mut changed = fixture.request.clone();
    changed.name = "another_tool".into();
    assert_eq!(
        authorize(&mut fixture.database, fixture.turn, &changed, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
        .await
        .unwrap();
    assert_eq!(
        authorize(&mut fixture.database, fixture.turn, &fixture.request, None)
            .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn rejects_invalid_decisions() {
    let mut fixture = Fixture::new().await;
    let mut changed = fixture.request.clone();
    changed.arguments["path"] = json!("another.txt");
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &changed,
            None,
            &fixture.events
        )
        .await
        .is_err()
    );
    let approval = fixture.begin().await;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: fixture.turn },
    )
    .await;
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
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
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
    .await
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, approval.id)
            .await
            .unwrap()
            .state,
        ApprovalState::Cancelled
    );
}

#[tokio::test]
async fn closes_pending_on_restart() {
    let mut fixture = Fixture::new().await;
    let approval = fixture.begin().await;
    let node = fixture.database.node;
    fixture.database.close().await.unwrap();
    let database = Database::open(&fixture.path, node, None).await.unwrap();
    assert_eq!(
        get(&database.connection, approval.id).await.unwrap().state,
        ApprovalState::Interrupted
    );
    assert_eq!(
        runs::get(&database.connection, fixture.turn)
            .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.approvals[0].id, approval.id);
}
