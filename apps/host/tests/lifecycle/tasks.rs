use super::*;
use sailry_client::Client;
use sailry_link::{CancellationToken, Link, NetworkScope};
use sailry_protocol::{
    Authentication, Command as NodeCommand, Effort, Output, Permission, ProviderId, RequestId,
    Session, SessionConfig, SessionId, TurnId, WorkMode,
    conversation::{self, ApprovalState, ModelApi, Provider, Status},
};
use serde_json::Value;
use std::path::PathBuf;

use super::workload::models;
#[path = "tasks/fixture.rs"]
pub(super) mod fixture;
use fixture::{Fixture, management, result};

#[test]
fn requires_a_running_host_without_creating_data() {
    let directory = tempdir().unwrap();
    let profile = directory.path().join("never started");
    let request = RequestId::new();
    let session = SessionId::new();
    let output = management(&profile, "submit")
        .args([
            "--session",
            &session.to_string(),
            "--request-id",
            &request.to_string(),
            "--prompt",
            "A task",
        ])
        .output()
        .unwrap();
    let json = result(output, 1);
    assert_eq!(json["state"], "failed");
    assert_eq!(json["request"], serde_json::to_value(request).unwrap());
    assert_eq!(json["session"], serde_json::to_value(session).unwrap());
    assert!(json["node"].is_null());
    assert_eq!(json["error"]["code"], "unavailable");
    assert!(!profile.exists());
    let help = management(&profile, "submit")
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("Exit codes:")
    );
    assert!(!profile.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn submits_and_retries_through_the_existing_owner() {
    let mut fixture =
        Fixture::new(models::Server::markdown("CI result 中文 🙂".into()).await).await;
    let request = RequestId::new();
    let prompt = "Keep literal `text` and $(touch escaped.txt) with 中文 🙂";
    let task = fixture.submit(request, prompt);
    let completed = fixture.invoke(task).await;
    let completed = result(completed, 0);
    fixture.assert_identity(&completed, request);
    assert_eq!(completed["state"], "completed");
    assert_eq!(completed["text"], "CI result 中文 🙂");
    let turn: TurnId = serde_json::from_value(completed["turn"].clone()).unwrap();
    assert!(!fixture.root.join("escaped.txt").exists());
    let repeated = result(fixture.invoke(fixture.submit(request, prompt)).await, 0);
    assert_eq!(repeated["turn"], completed["turn"]);
    let changed = result(
        fixture
            .invoke(fixture.submit(request, "Different input"))
            .await,
        1,
    );
    assert_eq!(changed["error"]["code"], "conflict");
    let observed = result(fixture.invoke(fixture.wait(turn, 10)).await, 0);
    assert_eq!(observed["state"], "completed");
    assert_eq!(observed["turn"], completed["turn"]);
    assert_eq!(fixture.page().await.runs.len(), 1);
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 1);
    assert!(fixture.host.child.try_wait().unwrap().is_none());
    assert!(Node::start(&fixture.profile).await.is_err());
    fixture.host.stop();
    let restored = Node::start_isolated(&fixture.profile).await.unwrap();
    let client = Client::new(restored.local());
    assert_eq!(
        client
            .read_conversation(fixture.session.id, None, 100)
            .await
            .unwrap()
            .page
            .runs
            .len(),
        1
    );
    restored.shutdown().await.unwrap();
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retries_implicit_revision_after_configuration_changes() {
    let mut fixture = Fixture::new(models::Server::scripted_tools(Vec::new()).await).await;
    let request = RequestId::new();
    let prompt = "Complete this task once";
    let completed = result(
        fixture
            .invoke(fixture.submit_current(request, prompt))
            .await,
        0,
    );
    fixture.assert_identity(&completed, request);
    assert_eq!(completed["state"], "completed");
    let mut config = fixture.session.config.clone();
    config.mode = WorkMode::Plan;
    let Output::Session(updated) = fixture
        .execute(NodeCommand::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        })
        .await
    else {
        panic!("session expected")
    };
    assert_eq!(updated.revision, fixture.session.revision + 1);
    let repeated = result(
        fixture
            .invoke(fixture.submit_current(request, prompt))
            .await,
        0,
    );
    fixture.assert_identity(&repeated, request);
    assert_eq!(repeated["state"], "completed");
    assert_eq!(repeated["turn"], completed["turn"]);
    assert_eq!(repeated["text"], completed["text"]);
    let changed = result(
        fixture
            .invoke(fixture.submit_current(request, "Changed task input"))
            .await,
        1,
    );
    assert_eq!(changed["error"]["code"], "conflict");
    let proposal = conversation::question::Spec {
        prompt: "Review the requested plan".into(),
        input: conversation::question::Input::Plan,
    };
    fixture.model.replace_tools(vec![(
        "ask_user".into(),
        serde_json::to_value(&proposal).unwrap(),
    )]);
    let next_request = RequestId::new();
    let next = result(
        fixture
            .invoke(fixture.submit_current(next_request, prompt))
            .await,
        3,
    );
    assert_eq!(next["state"], "awaiting_input");
    assert_eq!(next["request"], serde_json::to_value(next_request).unwrap());
    assert_eq!(next["node"], completed["node"]);
    assert_eq!(next["session"], completed["session"]);
    assert_eq!(next["revision"], updated.revision);
    assert_ne!(next["turn"], completed["turn"]);
    let questions: Vec<conversation::question::Question> =
        serde_json::from_value(next["questions"].clone()).unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].state, conversation::question::State::Pending);
    assert_eq!(questions[0].session, fixture.session.id);
    assert_eq!(
        serde_json::to_value(questions[0].turn).unwrap(),
        next["turn"]
    );
    let page = fixture.page().await;
    assert_eq!(page.questions, questions);
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == questions[0].entry)
        .unwrap();
    assert_eq!(entry.author, "assistant");
    let conversation::Part::ToolCall { name, summary, .. } = &entry.parts[questions[0].index]
    else {
        panic!("question tool call expected")
    };
    assert_eq!(name, "ask_user");
    assert_eq!(summary.question.as_deref(), Some(&proposal));
    let stale = result(
        fixture
            .invoke(fixture.submit(RequestId::new(), prompt))
            .await,
        1,
    );
    assert_eq!(stale["error"]["code"], "revision_conflict");
    assert_eq!(fixture.page().await.runs.len(), 2);
    assert_eq!(fixture.model.requests.lock().unwrap().len(), 2);
    assert!(fixture.host.child.try_wait().unwrap().is_none());
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_leaves_the_agent_running() {
    let release = CancellationToken::new();
    let fixture = Fixture::new(
        models::Server::markdown_held("Continues after timeout".into(), release.clone()).await,
    )
    .await;
    let request = RequestId::new();
    let mut submit = fixture.submit(request, "Complete after the observer leaves");
    submit.arg("--no-wait");
    let admitted = result(fixture.invoke(submit).await, 0);
    assert_eq!(admitted["state"], "admitted");
    fixture.assert_identity(&admitted, request);
    let turn = serde_json::from_value(admitted["turn"].clone()).unwrap();
    fixture.model.wait_count(1).await;
    let timed_out = result(fixture.invoke(fixture.wait(turn, 1)).await, 124);
    assert_eq!(timed_out["state"], "timed_out");
    assert_eq!(timed_out["turn"], admitted["turn"]);
    assert_eq!(timed_out["revision"], admitted["revision"]);
    assert_eq!(fixture.page().await.runs[0].status, Status::Running);
    release.cancel();
    let completed = result(fixture.invoke(fixture.wait(turn, 10)).await, 0);
    assert_eq!(completed["state"], "completed");
    assert_eq!(completed["text"], "Continues after timeout");
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reports_an_explicitly_stopped_turn() {
    let fixture = Fixture::new(
        models::Server::markdown_held("Stopped answer".into(), CancellationToken::new()).await,
    )
    .await;
    let mut submit = fixture.submit(RequestId::new(), "Keep working until explicitly stopped");
    submit.arg("--no-wait");
    let admitted = result(fixture.invoke(submit).await, 0);
    let turn = serde_json::from_value(admitted["turn"].clone()).unwrap();
    fixture.model.wait_count(1).await;
    fixture.execute(NodeCommand::StopTurn { turn }).await;
    let stopped = result(fixture.invoke(fixture.wait(turn, 10)).await, 1);
    assert_eq!(stopped["state"], "cancelled");
    assert_eq!(stopped["turn"], admitted["turn"]);
    assert_eq!(fixture.page().await.runs[0].status, Status::Cancelled);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn creates_a_worktree_task_from_stdin() {
    let fixture = Fixture::new(models::Server::start(false).await).await;
    let request = RequestId::new();
    let config = fixture.directory.path().join("task config.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&fixture.session.config).unwrap(),
    )
    .unwrap();
    let mut command = management(&fixture.profile, "submit");
    command
        .args([
            "--worktree",
            &fixture.session.worktree.to_string(),
            "--request-id",
            &request.to_string(),
            "--title",
            "CI task",
            "--stdin",
            "--timeout",
            "10",
        ])
        .arg("--config")
        .arg(&config);
    let input = "Task from stdin 中文 🙂\nPreserve every literal $(expression)";
    let completed = result(fixture.invoke_with_input(command, input).await, 0);
    assert_eq!(completed["state"], "completed");
    assert_eq!(completed["request"], serde_json::to_value(request).unwrap());
    let session: SessionId = serde_json::from_value(completed["session"].clone()).unwrap();
    assert_ne!(session, fixture.session.id);
    let Output::Session(created) = fixture.execute(NodeCommand::ReadSession { session }).await
    else {
        panic!("session expected")
    };
    assert_eq!(created.config, fixture.session.config);
    assert_eq!(created.worktree, fixture.session.worktree);
    assert_eq!(created.activity.title, "CI task");
    assert_eq!(completed["revision"], created.revision);
    assert_eq!(
        fixture
            .client
            .read_conversation(session, None, 100)
            .await
            .unwrap()
            .page
            .runs
            .len(),
        1
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn returns_approval_without_autoapproval() {
    let fixture = Fixture::new(models::Server::tools(vec![(models::plugin_tool("files", "write_file"),
        serde_json::json!({"path":"blocked.txt","text":"Needs consent","expected_revision":null}))]).await).await;
    let request = RequestId::new();
    let blocked = result(
        fixture
            .invoke(fixture.submit(request, "Write the requested file"))
            .await,
        3,
    );
    fixture.assert_identity(&blocked, request);
    assert_eq!(blocked["state"], "awaiting_approval");
    let approvals: Vec<conversation::Approval> =
        serde_json::from_value(blocked["approvals"].clone()).unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].state, ApprovalState::Pending);
    assert!(!fixture.root.join("blocked.txt").exists());
    assert_eq!(fixture.page().await.runs[0].status, Status::Running);
    fixture
        .execute(NodeCommand::ResolveApproval {
            session: approvals[0].session,
            approval: approvals[0].id,
            decision: conversation::Decision::Deny,
        })
        .await;
    let turn = serde_json::from_value(blocked["turn"].clone()).unwrap();
    assert_eq!(
        result(fixture.invoke(fixture.wait(turn, 10)).await, 0)["state"],
        "completed"
    );
    assert!(!fixture.root.join("blocked.txt").exists());
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ci_uses_an_already_configured_host() {
    let fixture = Fixture::new(models::Server::start(false).await).await;
    let request = RequestId::new();
    let prompt = fixture.directory.path().join("CI prompt 中文.txt");
    let text =
        "Treat $(touch injected.txt) and `touch altered.txt` as literal text\nComplete the CI task";
    std::fs::write(&prompt, text).unwrap();
    let mut command = Command::new("bash");
    command
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/ci-task.sh"))
        .env("SAILRY_COMMAND", fixture::binary())
        .env("SAILRY_REQUEST_ID", request.to_string())
        .env("SAILRY_SESSION", fixture.session.id.to_string())
        .env(
            "SAILRY_EXPECTED_REVISION",
            fixture.session.revision.to_string(),
        )
        .env("SAILRY_PROFILE", &fixture.profile)
        .env("SAILRY_PROMPT_FILE", &prompt)
        .env_remove("SAILRY_WORKTREE")
        .env_remove("SAILRY_CONFIG")
        .stdin(Stdio::null());
    let completed = result(fixture.invoke(command).await, 0);
    fixture.assert_identity(&completed, request);
    assert_eq!(completed["state"], "completed");
    let page = fixture.page().await;
    assert!(
        page.entries
            .iter()
            .filter(|entry| entry.author == "user")
            .any(|entry| entry
                .parts
                .iter()
                .any(|part| matches!(part, conversation::Part::Text(value) if value == text)))
    );
    assert!(!fixture.root.join("injected.txt").exists());
    assert!(!fixture.root.join("altered.txt").exists());
    assert_eq!(page.runs.len(), 1);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reports_disconnect_and_interrupt_honestly() {
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        net::UnixListener,
    };
    for interrupt in [false, true] {
        let directory = tempdir().unwrap();
        let listener = UnixListener::bind(directory.path().join("control.sock")).unwrap();
        let request = RequestId::new();
        let session = SessionId::new();
        let child = management(directory.path(), "submit")
            .args([
                "--session",
                &session.to_string(),
                "--request-id",
                &request.to_string(),
                "--expected-revision",
                "7",
                "--prompt",
                "Observe safely",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (stream, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(10), stream.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let sent: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(sent["request"], serde_json::to_value(request).unwrap());
        if interrupt {
            assert!(
                Command::new("kill")
                    .args(["-INT", &child.id().to_string()])
                    .status()
                    .unwrap()
                    .success()
            );
        } else {
            drop(stream);
        }
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || child.wait_with_output().unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        let json = result(output, if interrupt { 130 } else { 4 });
        assert_eq!(
            json["state"],
            if interrupt {
                "observation_cancelled"
            } else {
                "uncertain"
            }
        );
        assert_eq!(json["request"], serde_json::to_value(request).unwrap());
        assert_eq!(json["session"], serde_json::to_value(session).unwrap());
        assert_eq!(json["revision"], 7);
        assert!(json["turn"].is_null());
        assert!(!directory.path().join("node.lock").exists());
    }
}
