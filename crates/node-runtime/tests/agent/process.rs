#![cfg(unix)]
use super::*;
use sailry_protocol::process::{Completion, Outcome};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(super) struct Fixture {
    pub(super) directory: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) node: Node,
    pub(super) controller: Link,
    pub(super) client: Client,
    pub(super) session: Session,
}

impl Fixture {
    pub(super) async fn new(remote: bool, server: &Server) -> Self {
        Self::setup(remote, server, None).await
    }

    pub(super) async fn files(remote: bool, server: &Server) -> Self {
        Self::packages(remote, server, &["files"]).await
    }

    pub(super) async fn packages(remote: bool, server: &Server, packages: &[&str]) -> Self {
        Self::setup(remote, server, Some(packages)).await
    }

    async fn setup(remote: bool, server: &Server, packages: Option<&[&str]>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = if packages.is_some() {
            Node::start_empty(directory.path().join("node")).await
        } else {
            Node::start(directory.path().join("node")).await
        }
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let session = if let Some(packages) = packages {
            approvals::prepare_packages(&client, server, &root, packages).await
        } else {
            let session = approvals::prepare(&client, server, &root).await;
            isolate_model_tools(&client).await;
            session
        };
        Self {
            directory,
            root,
            node,
            controller,
            client,
            session,
        }
    }

    fn command(&self, turn: TurnId, command: &str) -> Request {
        self.client.prepare(Command::RunCommand {
            turn,
            command: command.into(),
            access: sailry_protocol::process::Access::Unrestricted,
            description: None,
            cwd: String::new(),
            timeout_ms: 60_000,
            pty: false,
            background: false,
            attachments: Vec::new(),
        })
    }
}

pub(super) async fn decide(
    client: &Client,
    approval: &Approval,
    decision: Decision,
) -> (Request, Output) {
    let request = client.prepare(Command::ResolveApproval {
        session: approval.session,
        approval: approval.id,
        decision,
    });
    let response = client.execute(request.clone()).await.unwrap();
    (request, response)
}

pub(super) fn results(page: &Page) -> Vec<Completion> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| {
            if let Part::ToolResult { result, .. } = part
                && let Ok(Output::CommandResult(result)) = serde_json::from_value(result.clone())
            {
                Some(result)
            } else {
                None
            }
        })
        .collect()
}

async fn child(path: &Path) -> i32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(path)
                && let Ok(pid) = text.parse()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture child did not start")
}

async fn gone(pid: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        // SAFETY: signal 0 only probes the exact fixture PID and passes no pointers.
        while unsafe { libc::kill(pid, 0) } == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture child did not exit");
}

#[tokio::test]
async fn reuses_turn_scratch() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command": "test \"$TMPDIR\" = \"$TMP\" && test \"$TMPDIR\" = \"$TEMP\" && printf scratch > \"$TMPDIR/shared.txt\" && printf '%s' \"$TMPDIR\" > scratch-first.txt"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command": "test \"$(cat \"$TMPDIR/shared.txt\")\" = scratch && printf '%s' \"$TMPDIR\" > scratch-second.txt && printf reused"}),
            ),
        ])
        .await;
        let fixture = Fixture::packages(remote, &server, &["commands"]).await;
        let session = fixture.session.id;
        let turn = approvals::submit(&fixture.client, session).await;
        for _ in 0..2 {
            let (_, approval) = approvals::pending(&fixture.client, session).await;
            decide(&fixture.client, &approval, Decision::Approve).await;
        }
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].outcome, Outcome::Exited(0));
        assert_eq!(output[1].outcome, Outcome::Exited(0));
        assert_eq!(output[1].stdout.text, "reused");
        let first = std::fs::read_to_string(fixture.root.join("scratch-first.txt")).unwrap();
        let second = std::fs::read_to_string(fixture.root.join("scratch-second.txt")).unwrap();
        assert_eq!(first, second);
        let scratch = Path::new(&first);
        assert_eq!(
            scratch.parent(),
            Some(fixture.node.profile().join("tmp").as_path())
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while scratch.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("finished turn retained its temporary storage");
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn bounds_approved_results() {
    for remote in [false, true] {
        let command = "printf '批准的内容 🙂' > created.txt; printf '输出中文 🙂'; printf 'error text' >&2; exit 7";
        let server = Server::tools(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command": command, "cwd": "项目", "timeout_ms": 1000}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command": "printf 'before timeout'; sleep 60", "timeout_ms": 100}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command": "touch denied.txt"}),
            ),
        ])
        .await;
        let fixture = Fixture::packages(remote, &server, &["commands"]).await;
        std::fs::create_dir(fixture.root.join("项目")).unwrap();
        let session = fixture.session.id;
        let turn = approvals::submit(&fixture.client, session).await;
        let (_, first) = approvals::pending(&fixture.client, session).await;
        assert!(!fixture.root.join("项目/created.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let (decision, receipt) = decide(&fixture.client, &first, Decision::Approve).await;
        let (_, second) = approvals::pending(&fixture.client, session).await;
        assert_ne!(first.id, second.id);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("项目/created.txt")).unwrap(),
            "批准的内容 🙂"
        );
        decide(&fixture.client, &second, Decision::Approve).await;
        let (_, third) = approvals::pending(&fixture.client, session).await;
        decide(&fixture.client, &third, Decision::Deny).await;
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].outcome, Outcome::Exited(7));
        assert_eq!(output[0].stdout.text, "输出中文 🙂");
        assert_eq!(output[0].stderr.text, "error text");
        assert_eq!(output[1].outcome, Outcome::TimedOut);
        assert_eq!(output[1].stdout.text, "before timeout");
        assert!(!fixture.root.join("denied.txt").exists());
        assert_eq!(
            fixture.client.execute(decision.clone()).await.unwrap(),
            receipt
        );
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let id: String = database
            .query_row(
                "SELECT request FROM agent_approvals WHERE id=?1",
                [first.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let Output::Plugin(package) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "commands".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("package expected")
        };
        let request = Request {
            id: id.parse().unwrap(),
            ..fixture
                .client
                .prepare(Command::RunCommand {
                    turn,
                    command: command.into(),
                    access: sailry_protocol::process::Access::Unrestricted,
                    description: None,
                    cwd: "项目".into(),
                    timeout_ms: 1000,
                    pty: false,
                    background: false,
                    attachments: Vec::new(),
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: Some(turn),
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(fixture.session.worktree),
                    session: Some(session),
                })
        };
        let body: Vec<u8> = database
            .query_row("SELECT body FROM requests WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(body.starts_with(b"command-v1:"));
        drop(database);
        let expected = Output::CommandResult(output[0].clone());
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            expected
        );
        assert_eq!(
            fixture.client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(expected.clone())))
        );
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, session).await, page);
        assert_eq!(client.execute(request).await.unwrap(), expected);
        assert_eq!(client.execute(decision).await.unwrap(), receipt);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_reaps_commands() {
    for remote in [false, true] {
        for shutdown in [false, true] {
            let server = Server::start(true).await;
            let fixture = Fixture::packages(remote, &server, &["commands"]).await;
            let session = fixture.session.id;
            let turn = approvals::submit(&fixture.client, session).await;
            server.wait_count(1).await;
            let request = fixture.command(turn, "sleep 60 & task_pid=$!; printf '%s' \"$task_pid\" > child.pid; printf 'partial output'; wait \"$task_pid\"");
            let admission = fixture.client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let pid = child(&fixture.root.join("child.pid")).await;
            assert_eq!(
                fixture.client.outcome(&request).await.unwrap(),
                RequestOutcome::Admitted
            );
            assert_eq!(
                fixture
                    .client
                    .execute(request.clone())
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::OutcomeUnknown
            );
            drop(admission);
            tokio::time::timeout(Duration::from_secs(2), async {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::Snapshot))
                    .await
                    .unwrap();
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::WriteFile {
                        worktree: fixture.session.worktree,
                        path: "probe.txt".into(),
                        text: "available".into(),
                        expected_revision: None,
                    }))
                    .await
                    .unwrap();
            })
            .await
            .expect("command blocked storage or file operations");
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("child.pid")).unwrap(),
                pid.to_string()
            );
            if !shutdown {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn { turn }))
                    .await
                    .unwrap();
                let page = finished(&fixture.client, session, turn).await;
                assert_eq!(page.runs[0].status, Status::Cancelled);
                gone(pid).await;
            }
            fixture.node.shutdown().await.unwrap();
            gone(pid).await;
            let node = Node::start_empty(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let Output::CommandResult(result) = client.execute(request.clone()).await.unwrap()
            else {
                panic!("command result expected")
            };
            assert_eq!(result.outcome, Outcome::Cancelled);
            assert_eq!(result.stdout.text, "partial output");
            let page = history(&client, session).await;
            assert_eq!(
                page.runs[0].status,
                if shutdown {
                    Status::Interrupted
                } else {
                    Status::Cancelled
                }
            );
            let mut changed = request.clone();
            if let Command::RunCommand { command, .. } = &mut changed.command {
                *command = "touch replayed.txt".into();
            }
            assert_eq!(
                client.execute(changed).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(
                client
                    .execute(Request {
                        id: RequestId::new(),
                        ..request
                    })
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert!(!fixture.root.join("replayed.txt").exists());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn orders_batch_results() {
    for remote in [false, true] {
        let server = Server::batch(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":"printf first > first.txt; sleep 1; printf first"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":"printf second > second.txt"}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path":"first.txt"}),
            ),
        ])
        .await;
        let fixture = Fixture::packages(remote, &server, &["commands", "files"]).await;
        let session = fixture.session.id;
        let turn = approvals::submit(&fixture.client, session).await;
        let (page, first) = approvals::pending(&fixture.client, session).await;
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::ToolCall { .. })))
                .count(),
            1,
            "fixture must emit a single batch"
        );
        assert!(results(&page).is_empty());
        assert!(!fixture.root.join("first.txt").exists());
        decide(&fixture.client, &first, Decision::Approve).await;
        let running = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let page = history(&fixture.client, session).await;
                if page.entries.iter().flat_map(|entry| &entry.parts).any(
                    |part| matches!(part, Part::Resource(value) if value["type"] == "tool_started"),
                ) {
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(results(&running).is_empty());
        assert_eq!(running.approvals.len(), 1);
        let (page, second) = approvals::pending(&fixture.client, session).await;
        assert_ne!(first.id, second.id);
        assert_eq!(
            results(&page).len(),
            1,
            "first result must be durable before second approval"
        );
        assert_eq!(results(&page)[0].stdout.text, "first");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("first.txt")).unwrap(),
            "first"
        );
        assert!(!fixture.root.join("second.txt").exists());
        decide(&fixture.client, &second, Decision::Deny).await;
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let calls = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolCall { .. }))
            .count();
        let responses = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolResult { .. }))
            .count();
        assert_eq!((calls, responses), (3, 3));
        assert_eq!(results(&page).len(), 1);
        assert!(!fixture.root.join("second.txt").exists());
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn delivers_completion_without_consuming_controller_reads() {
    use sailry_protocol::conversation::background::{Kind, State};
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let server = Server::markdown_held("Parent finished".into(), release.clone()).await;
        let fixture = Fixture::packages(remote, &server, &[]).await;
        let Output::QueuedTurn(parent) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Keep the existing task active".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(1).await;
        let command = fixture.client.prepare(Command::RunCommand {
            turn: parent.id,
            command: "printf 'Completed background evidence'".into(),
            description: Some("Background fixture".into()),
            access: sailry_protocol::process::Access::Workspace,
            cwd: String::new(),
            timeout_ms: 10_000,
            background: true,
            pty: false,
            attachments: Vec::new(),
        });
        let Output::CommandStarted(info) = fixture.client.execute(command.clone()).await.unwrap()
        else {
            panic!("started command expected")
        };
        let id = format!("command:{}", info.id);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .background
                    .iter()
                    .any(|task| task.id == id && task.state == State::Pending)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("finished command must persist while the parent is busy");
        for _ in 0..3 {
            let Output::CommandOutput(snapshot) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCommand {
                    session: fixture.session.id,
                    id: info.id,
                }))
                .await
                .unwrap()
            else {
                panic!("command snapshot expected")
            };
            assert_eq!(snapshot.stdout.text, "Completed background evidence");
            assert!(matches!(
                snapshot.info.status,
                sailry_protocol::process::Status::Finished(Outcome::Exited(0))
            ));
        }
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(
            page.background
                .iter()
                .find(|task| task.id == id)
                .unwrap()
                .state,
            State::Pending
        );
        assert!(page.runs.iter().all(|run| !run.notice));
        release.cancel();
        let page = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.session.id).await;
                if page
                    .runs
                    .iter()
                    .any(|run| run.notice && run.status == Status::Completed)
                {
                    break page;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("idle parent must receive the completed command");
        let task = page.background.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, State::Delivered);
        assert_eq!(task.kind, Kind::Command { id: info.id });
        let notice = page.runs.iter().find(|run| run.notice).unwrap();
        assert_eq!(task.delivery, Some(notice.turn));
        assert_eq!(notice.notices.len(), 1);
        assert_eq!(notice.notices[0].id, id);
        assert!(notice.input.is_none());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let request = server.requests.lock().unwrap().last().unwrap().clone();
        let content = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|message| message["role"] == "user")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let data: serde_json::Value =
            serde_json::from_str(content.split_once('\n').unwrap().1).unwrap();
        assert_eq!(
            data[0]["output"]["stdout_tail"],
            "Completed background evidence"
        );
        // The original durable request remains an idempotent start receipt.
        assert_eq!(
            fixture.client.execute(command).await.unwrap(),
            Output::CommandStarted(info)
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
