use super::*;
use sailry_link::{Link, NetworkScope};
use sailry_protocol::conversation::{ApprovalState, Status};
use std::time::Duration;

mod fixture;
mod transport;
use fixture::Fixture;

#[test]
fn accepts_only_current_envelopes() {
    let task = Task::Wait {
        version: 1,
        session: SessionId::new(),
        turn: TurnId::new(),
        timeout: 10,
    };
    let value = serde_json::to_value(&task).unwrap();
    assert!(parse(&format!("{value}\n")).is_ok());
    for field in ["version", "timeout", "unexpected"] {
        let mut value = value.clone();
        value[field] = serde_json::json!(if field == "version" { 2 } else { 0 });
        assert!(parse(&format!("{value}\n")).is_err());
    }
    assert!(parse(&value.to_string()).is_err());
    assert!(parse(&format!("{}\n", " ".repeat(MAX_REQUEST))).is_err());
}

#[test]
fn keeps_response_identity() {
    let mut known = Result::new(Some(NodeId([1; 32])));
    known.request = Some(RequestId::new());
    known.session = Some(SessionId::new());
    known.turn = Some(TurnId::new());
    known.revision = Some(7);
    for field in 0..6 {
        let mut changed = known.clone();
        match field {
            0 => changed.version = 2,
            1 => changed.node = Some(NodeId([2; 32])),
            2 => changed.request = Some(RequestId::new()),
            3 => changed.session = Some(SessionId::new()),
            4 => changed.turn = Some(TurnId::new()),
            _ => changed.revision = Some(8),
        }
        assert!(!matches_response(&known, &changed));
    }
    known.state = State::Completed;
    assert!(matches_response(&known, &known));
    assert_eq!(known.exit_code(), 0);
    known.state = State::AwaitingApproval;
    assert_eq!(known.exit_code(), 3);
    known.state = State::TimedOut;
    assert_eq!(known.exit_code(), 124);
    known.state = State::ObservationCancelled;
    assert_eq!(known.exit_code(), 130);
    assert_eq!(uncertain(known, "missing observation").exit_code(), 4);
}

#[tokio::test]
async fn admits_exactly_one_durable_turn() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            fixture::models::Server::markdown("Complete 中文 🙂".into()).await,
        )
        .await;
        let request = RequestId::new();
        let task = fixture.submit(request, false);
        let admitted = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(admitted.state, State::Admitted);
        assert_eq!(admitted.request, Some(request));
        assert_eq!(admitted.node, Some(fixture.node.id()));
        assert_eq!(admitted.session, Some(fixture.session.id));
        assert_eq!(admitted.revision, Some(fixture.session.revision));
        let turn = admitted.turn.unwrap();
        let completed = fixture.wait(turn, 10).await;
        assert_eq!(completed.state, State::Completed);
        assert_eq!(completed.text, "Complete 中文 🙂");
        let repeated = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(repeated.turn, Some(turn));
        assert_eq!(fixture.page().await.runs.len(), 1);
        let mut changed = fixture.submit(request, false);
        let Task::Submit { message, .. } = &mut changed else {
            unreachable!()
        };
        message.text = "Different request payload".into();
        let rejected = fixture.exchange(&changed).await.pop().unwrap();
        assert_eq!(rejected.state, State::Failed);
        assert_eq!(rejected.error.unwrap().code, ErrorCode::Conflict);
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        assert!(
            sailry_node_runtime::Node::start(fixture.node.profile())
                .await
                .is_err()
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn creates_and_admits_atomically() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, fixture::models::Server::start(false).await).await;
        let request = RequestId::new();
        let task = Task::Submit {
            version: 1,
            request,
            target: Target::Worktree {
                worktree: fixture.session.worktree,
                config: Some(Box::new(fixture.session.config.clone())),
            },
            message: "Start a new registered worktree task".into(),
            title: "Atomic task".into(),
            wait: true,
            timeout: 10,
        };
        let responses = fixture.exchange(&task).await;
        assert_eq!(responses.len(), 2);
        let admitted = &responses[0];
        let completed = &responses[1];
        assert_eq!(admitted.state, State::Admitted);
        assert_eq!(completed.state, State::Completed);
        assert_eq!(completed.request, Some(request));
        assert_ne!(completed.session, Some(fixture.session.id));
        assert_eq!(completed.turn, admitted.turn);
        let Output::Session(session) = fixture
            .execute(Command::ReadSession {
                session: completed.session.unwrap(),
            })
            .await
        else {
            panic!("session expected")
        };
        assert_eq!(session.config, fixture.session.config);
        assert_eq!(session.worktree, fixture.session.worktree);
        assert_eq!(session.activity.title, "Atomic task");
        let repeated = fixture.exchange(&task).await;
        assert_eq!(repeated.last().unwrap().session, completed.session);
        assert_eq!(repeated.last().unwrap().turn, completed.turn);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn reports_approval_without_granting_it() {
    for remote in [false, true] {
        let model = fixture::models::Server::tools(vec![(fixture::models::plugin_tool("files", "write_file"),
            serde_json::json!({"path":"protected.txt","text":"Requires approval","expected_revision":null}))]).await;
        let fixture = Fixture::new(remote, model).await;
        let responses = fixture
            .exchange(&fixture.submit(RequestId::new(), true))
            .await;
        let blocked = responses.last().unwrap();
        assert_eq!(blocked.state, State::AwaitingApproval);
        assert_eq!(blocked.exit_code(), 3);
        assert_eq!(blocked.approvals.len(), 1);
        let approval = &blocked.approvals[0];
        assert_eq!(approval.session, fixture.session.id);
        assert_eq!(approval.state, ApprovalState::Pending);
        assert!(!fixture.root().join("protected.txt").exists());
        assert_eq!(fixture.page().await.runs[0].status, Status::Running);
        fixture
            .execute(Command::ResolveApproval {
                session: approval.session,
                approval: approval.id,
                decision: conversation::Decision::Deny,
            })
            .await;
        assert_eq!(
            fixture.wait(blocked.turn.unwrap(), 10).await.state,
            State::Completed
        );
        assert!(!fixture.root().join("protected.txt").exists());
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_work_after_observation_ends() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let model =
            fixture::models::Server::markdown_held("Still completes".into(), release.clone()).await;
        let fixture = Fixture::new(remote, model).await;
        let admitted = fixture
            .exchange(&fixture.submit(RequestId::new(), false))
            .await
            .pop()
            .unwrap();
        let turn = admitted.turn.unwrap();
        fixture.model.wait_count(1).await;
        assert_eq!(fixture.wait(turn, 1).await.state, State::TimedOut);
        assert_eq!(fixture.page().await.runs[0].status, Status::Running);
        let mut waiting = fixture
            .connect(&Task::Wait {
                version: 1,
                session: fixture.session.id,
                turn,
                timeout: 10,
            })
            .await;
        assert_eq!(fixture::receive(&mut waiting).await.state, State::Observing);
        drop(waiting);
        fixture.node.observe().probe().await.unwrap();
        assert_eq!(fixture.page().await.runs[0].status, Status::Running);
        let stop = CancellationToken::new();
        stop.cancel();
        let cancelled = observe::wait(fixture.client.clone(), fixture.session.id, turn, 10, &stop)
            .await
            .unwrap();
        assert_eq!(cancelled.state, State::ObservationCancelled);
        assert_eq!(fixture.page().await.runs[0].status, Status::Running);
        release.cancel();
        let completed = fixture.wait(turn, 10).await;
        assert_eq!(completed.state, State::Completed);
        assert_eq!(completed.text, "Still completes");
        fixture.close().await;
    }
}

#[tokio::test]
async fn restores_an_older_admitted_turn() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, fixture::models::Server::start(false).await).await;
        let original = fixture
            .exchange(&fixture.submit(RequestId::new(), true))
            .await
            .pop()
            .unwrap();
        assert_eq!(original.state, State::Completed);
        crate::history_fixture::append(&fixture.client, &fixture.session, 24).await;
        let recent = fixture
            .client
            .read_conversation(fixture.session.id, None, 20)
            .await
            .unwrap();
        assert!(
            !recent
                .page
                .runs
                .iter()
                .any(|run| Some(run.turn) == original.turn)
        );
        let restored = fixture.wait(original.turn.unwrap(), 10).await;
        assert_eq!(restored.state, State::Completed);
        assert_eq!(restored.text, original.text);
        assert_eq!(restored.revision, original.revision);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 25);
        let absent = fixture.wait(TurnId::new(), 10).await;
        assert_eq!(absent.state, State::Uncertain);
        assert_eq!(absent.error.unwrap().code, ErrorCode::NotFound);
        fixture.close().await;
    }
}

#[tokio::test]
async fn reports_pending_plan_input() {
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote, fixture::models::Server::start(false).await).await;
        let mut config = fixture.session.config.clone();
        config.mode = WorkMode::Plan;
        let Output::Session(session) = fixture
            .execute(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            })
            .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let result = fixture
            .exchange(&fixture.submit(RequestId::new(), true))
            .await
            .pop()
            .unwrap();
        assert_eq!(result.state, State::AwaitingInput);
        assert_eq!(result.exit_code(), 3);
        assert_eq!(result.questions.len(), 1);
        assert_eq!(
            result.questions[0].state,
            conversation::question::State::Pending
        );
        let page = fixture.page().await;
        let turn = result.turn.unwrap();
        assert_eq!(result.questions[0].session, fixture.session.id);
        assert_eq!(result.questions[0].turn, turn);
        assert_eq!(page.questions, result.questions);
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            page.entries
                .iter()
                .find(|entry| entry.id == result.questions[0].entry)
                .unwrap()
                .author,
            "plan_review"
        );
        let repeated = fixture.wait(turn, 10).await;
        assert_eq!(repeated.state, State::AwaitingInput);
        assert_eq!(repeated.questions, result.questions);
        fixture.close().await;
    }
}

#[tokio::test]
async fn exposes_child_approval_to_the_observer() {
    for remote in [false, true] {
        let parent = fixture::models::Server::turn_tools(vec![(
            fixture::models::plugin_tool("delegation", "spawn_agent"),
            serde_json::json!({"role":"writer","task":"Write child.txt with the files tool"}),
        )])
        .await;
        let child = fixture::models::Server::turn_tools(vec![(fixture::models::plugin_tool("files", "write_file"),
            serde_json::json!({"path":"child.txt","text":"Delegated task","expected_revision":null}))]).await;
        let mut fixture = Fixture::new(remote, parent).await;
        let provider = fixture::provider(&fixture.client, &child.endpoint).await;
        let role = role::Profile {
            read_only: false,
            id: RoleId::new(),
            revision: 0,
            key: "writer".into(),
            name: "Writer".into(),
            appearance: None,
            description: "Write the requested fixture file".into(),
            model: Some(role::Model {
                provider: provider.id,
                model: "fixture".into(),
                effort: None,
            }),
            max_turns: Some(5),
            skills: Vec::new(),
            tools: None,
            instructions: "Use the files tool to complete the delegated task".into(),
        };
        let Output::Role(role) = fixture
            .execute(Command::PutRole {
                role,
                expected_revision: 0,
            })
            .await
        else {
            panic!("role expected")
        };
        let Output::Session(session) = fixture
            .execute(Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![role.reference()],
            })
            .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let result = fixture
            .exchange(&fixture.submit(RequestId::new(), true))
            .await
            .pop()
            .unwrap();
        assert_eq!(result.state, State::AwaitingApproval);
        assert_eq!(result.session, Some(fixture.session.id));
        assert_eq!(result.revision, Some(fixture.session.revision));
        assert_eq!(result.approvals.len(), 1);
        let approval = &result.approvals[0];
        assert_ne!(approval.session, fixture.session.id);
        assert_ne!(Some(approval.turn), result.turn);
        assert_eq!(approval.state, ApprovalState::Pending);
        assert!(fixture.page().await.approvals.is_empty());
        assert!(!fixture.root().join("child.txt").exists());
        fixture
            .execute(Command::ResolveApproval {
                session: approval.session,
                approval: approval.id,
                decision: conversation::Decision::Deny,
            })
            .await;
        assert_eq!(
            fixture.wait(result.turn.unwrap(), 10).await.state,
            State::Completed
        );
        assert!(!fixture.root().join("child.txt").exists());
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_identity_after_a_lost_admission() {
    for remote in [false, true] {
        let fixture = Fixture::with_loss(remote, fixture::models::Server::start(false).await).await;
        let request = RequestId::new();
        let task = fixture.submit(request, false);
        let missing = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(missing.state, State::Uncertain);
        assert_eq!(missing.exit_code(), 4);
        assert_eq!(missing.request, Some(request));
        assert_eq!(missing.session, Some(fixture.session.id));
        assert_eq!(missing.revision, Some(fixture.session.revision));
        assert!(missing.turn.is_none());
        assert_eq!(fixture.page().await.runs.len(), 1);
        let repeated = fixture.exchange(&task).await.pop().unwrap();
        let turn = repeated.turn.unwrap();
        assert_eq!(fixture.wait(turn, 10).await.state, State::Completed);
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn freezes_implicit_revision_before_retry() {
    for remote in [false, true] {
        let fixture = Fixture::with_loss(remote, fixture::models::Server::start(false).await).await;
        let request = RequestId::new();
        let mut task = fixture.submit(request, false);
        let Task::Submit {
            target: Target::Session {
                expected_revision, ..
            },
            ..
        } = &mut task
        else {
            panic!("session task expected")
        };
        *expected_revision = None;
        let missing = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(missing.state, State::Uncertain);
        assert_eq!(missing.exit_code(), 4);
        assert_eq!(missing.request, Some(request));
        assert_eq!(missing.session, Some(fixture.session.id));
        assert!(missing.revision.is_none());
        assert!(missing.turn.is_none());
        let original = fixture.page().await.runs[0].clone();
        assert_eq!(original.revision, fixture.session.revision);
        let controller = Link::controller(
            fixture.directory.path().join("revision-controller"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let address = controller
            .handle()
            .pair(fixture.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let other = Client::new(controller.handle().remote(address));
        let mut config = fixture.session.config.clone();
        config.mode = WorkMode::Plan;
        let Output::Session(updated) = other
            .execute(other.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(updated.revision, fixture.session.revision + 1);
        let recovered = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(recovered.state, State::Admitted);
        assert_eq!(recovered.request, Some(request));
        assert_eq!(recovered.turn, Some(original.turn));
        assert_eq!(recovered.revision, Some(original.revision));
        assert_eq!(
            fixture.wait(original.turn, 10).await.state,
            State::Completed
        );
        assert_eq!(fixture.page().await.runs.len(), 1);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
        let Task::Submit { message, .. } = &mut task else {
            unreachable!()
        };
        message.text = "Different input using the same request".into();
        let changed = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(changed.state, State::Failed);
        assert_eq!(changed.error.unwrap().code, ErrorCode::Conflict);
        assert_eq!(fixture.page().await.runs.len(), 1);
        let Task::Submit { request, .. } = &mut task else {
            unreachable!()
        };
        *request = RequestId::new();
        let current = fixture.exchange(&task).await.pop().unwrap();
        assert_eq!(current.state, State::Admitted);
        assert_eq!(current.revision, Some(updated.revision));
        assert_ne!(current.turn, Some(original.turn));
        let pending = fixture.wait(current.turn.unwrap(), 10).await;
        assert_eq!(pending.state, State::AwaitingInput);
        assert_eq!(pending.exit_code(), 3);
        assert_eq!(pending.revision, current.revision);
        assert_eq!(pending.turn, current.turn);
        assert_eq!(pending.questions.len(), 1);
        assert_eq!(
            pending.questions[0].state,
            conversation::question::State::Pending
        );
        assert_eq!(pending.questions[0].session, fixture.session.id);
        assert_eq!(Some(pending.questions[0].turn), current.turn);
        let page = fixture.page().await;
        assert_eq!(page.questions, pending.questions);
        assert_eq!(
            page.runs
                .iter()
                .find(|run| Some(run.turn) == current.turn)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            page.entries
                .iter()
                .find(|entry| entry.id == pending.questions[0].entry)
                .unwrap()
                .author,
            "plan_review"
        );
        let stale = fixture
            .exchange(&fixture.submit(RequestId::new(), false))
            .await
            .pop()
            .unwrap();
        assert_eq!(stale.state, State::Failed);
        assert_eq!(stale.error.unwrap().code, ErrorCode::RevisionConflict);
        assert_eq!(fixture.page().await.runs.len(), 2);
        assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
        controller.close().await.unwrap();
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_captured_plugin_submission() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, fixture::models::Server::start(false).await).await;
        let Output::Plugin(info) = fixture
            .execute(Command::ReadPlugin {
                name: "files".into(),
            })
            .await
        else {
            panic!("plugin expected")
        };
        for expected_revision in [None, Some(fixture.session.revision)] {
            let request = fixture
                .client
                .prepare(Command::SubmitTask {
                    session: fixture.session.id,
                    expected_revision,
                    message: "A plugin cannot impersonate task control".into(),
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: None,
                    surface: plugin::desktop::Surface::Workspace,
                    package: info.summary.reference(),
                    worktree: Some(fixture.session.worktree),
                    session: Some(fixture.session.id),
                });
            let error = fixture.client.execute(request).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            assert_eq!(
                error.message,
                "task submission requires a native controller"
            );
        }
        assert!(fixture.page().await.runs.is_empty());
        assert!(fixture.model.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}

#[tokio::test]
async fn distinguishes_history_failure_from_task_failure() {
    for remote in [false, true] {
        let fixture = Fixture::with_loss(remote, fixture::models::Server::start(false).await).await;
        let request = RequestId::new();
        assert_eq!(
            fixture
                .exchange(&fixture.submit(request, false))
                .await
                .pop()
                .unwrap()
                .state,
            State::Uncertain
        );
        let original = fixture
            .exchange(&fixture.submit(request, true))
            .await
            .pop()
            .unwrap();
        assert_eq!(original.state, State::Completed);
        crate::history_fixture::append(&fixture.client, &fixture.session, 24).await;
        fixture.loss.as_ref().unwrap().fail_paging();
        let missing = fixture.wait(original.turn.unwrap(), 10).await;
        assert_eq!(missing.state, State::Uncertain);
        assert_eq!(missing.error.unwrap().code, ErrorCode::Unavailable);
        assert_eq!(missing.turn, original.turn);
        assert_eq!(missing.revision, original.revision);
        assert_eq!(
            fixture
                .page()
                .await
                .runs
                .iter()
                .find(|run| Some(run.turn) == original.turn)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            fixture.wait(original.turn.unwrap(), 10).await.state,
            State::Completed
        );
        fixture.close().await;
    }
}
