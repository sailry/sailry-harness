#![cfg(unix)]
use super::*;
use sailry_protocol::code::{self, Content, Query};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
#[path = "code/install.rs"]
mod install;
#[path = "code/live.rs"]
mod live;
#[path = "code/overlays.rs"]
mod overlays;
#[path = "code/sandbox.rs"]
mod sandbox;

struct Fixture {
    directory: tempfile::TempDir,
    root: PathBuf,
    bin: PathBuf,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
}

fn environment(
    directory: &std::path::Path,
    bin: &std::path::Path,
) -> sailry_node_runtime::EnvironmentFixture {
    sailry_node_runtime::EnvironmentFixture {
        home: directory.into(),
        shell: "/bin/false".into(),
        environment: BTreeMap::from([("PATH".into(), bin.as_os_str().to_owned())]),
        timeout: Duration::from_secs(1),
    }
}

impl Fixture {
    async fn new(remote: bool, server: &Server) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap().join("project");
        let bin = directory.path().canonicalize().unwrap().join("bin");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(root.join("main.rs"), "🙂 broken symbol\n").unwrap();
        std::fs::write(root.join("target.rs"), "target function\n").unwrap();
        let executable = bin.join("rust-analyzer");
        let python = sailry_node_runtime::CodeServerFixture::python();
        std::fs::write(
            &executable,
            format!(
                "#!{}\n{}",
                python.display(),
                include_str!("../../src/code/tests/server.py")
            ),
        )
        .unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let node = Node::start_with_environment(
            directory.path().join("node"),
            environment(directory.path(), &bin),
        )
        .await
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
        let session = approvals::prepare(&client, server, &root).await;
        isolate_model_tools(&client).await;
        Self {
            directory,
            root,
            bin,
            node,
            controller,
            client,
            session,
        }
    }
    async fn config(&mut self, mode: WorkMode, permission: Permission) {
        let mut config = self.session.config.clone();
        config.mode = mode;
        config.permission = permission;
        let Output::Session(session) = self
            .client
            .execute(self.client.prepare(Command::SetSessionConfig {
                session: self.session.id,
                expected_revision: self.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        self.session = session;
    }
    async fn submit(&self) -> TurnId {
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: "Inspect this code".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        turn.id
    }
    async fn settings(&self, args: Vec<String>, after_edit: bool) {
        let Output::CodeSettings(mut settings) = self
            .client
            .execute(self.client.prepare(Command::ReadCodeSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        settings.after_edit = after_edit;
        settings.servers.insert(
            "rust-analyzer".into(),
            code::Override {
                disabled: false,
                command: None,
                args: Some(args),
                config: None,
                environment: None,
            },
        );
        let expected_revision = settings.revision;
        self.client
            .execute(self.client.prepare(Command::SaveCodeSettings {
                settings,
                expected_revision,
            }))
            .await
            .unwrap();
    }
    async fn finish(&self, turn: TurnId) -> Page {
        if self.session.config.mode == WorkMode::Plan {
            questions::finish_plan(&self.client, self.session.id, turn).await
        } else {
            finished(&self.client, self.session.id, turn).await
        }
    }
    async fn read(&self, query: Query, overlay: Option<code::Overlay>) -> code::Result {
        let Output::Code(result) = self
            .client
            .execute(self.client.prepare(Command::ReadCode {
                worktree: self.session.worktree,
                query,
                overlay,
            }))
            .await
            .unwrap()
        else {
            panic!("code expected")
        };
        result
    }
}

fn results(page: &Page) -> Vec<&Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| {
            if let Part::ToolResult { result, .. } = part {
                Some(result)
            } else {
                None
            }
        })
        .collect()
}

#[tokio::test]
async fn shares_confined_queries_and_saved_change_notifications() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let query = Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        };
        let result = fixture.read(query.clone(), None).await;
        assert_eq!(result.status, code::Status::Stopped);
        assert!(!fixture.root.join("pid").exists());
        let startup = result.startup.unwrap();
        let prepared = fixture.client.prepare(Command::StartCodeServer {
            startup: startup.clone(),
        });
        let receipt = fixture.client.dispatch(prepared.clone()).await.unwrap();
        assert!(receipt.receipt.durable);
        let Output::CodeServer(running) = receipt.completion.await.unwrap().unwrap() else {
            panic!("server expected")
        };
        assert_eq!(running.startup, startup);
        let repeated = fixture.client.execute(prepared).await.unwrap();
        assert!(matches!(repeated, Output::CodeServer(_)));
        let Content::Diagnostics(items) = fixture.read(query.clone(), None).await.content else {
            panic!("diagnostics expected")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].location.start.column, 3);
        for query in [
            Query::Definition {
                path: "main.rs".into(),
                line: 1,
                symbol: "symbol".into(),
            },
            Query::References {
                path: "main.rs".into(),
                line: 1,
                symbol: "symbol".into(),
            },
        ] {
            let Content::Locations(items) = fixture.read(query, None).await.content else {
                panic!("locations expected")
            };
            assert_eq!(items[0].path, "target.rs");
        }
        let saved = fixture
            .read(
                Query::Diagnostics {
                    path: "main.rs".into(),
                    severity: None,
                },
                None,
            )
            .await
            .revision
            .unwrap();
        let overlay = code::Overlay {
            path: "main.rs".into(),
            expected_revision: saved.clone(),
            text: "symbol is valid\n".into(),
        };
        let Content::Diagnostics(items) = fixture.read(query.clone(), Some(overlay)).await.content
        else {
            panic!("diagnostics expected")
        };
        assert!(items.is_empty());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::WriteFile {
                worktree: fixture.session.worktree,
                path: "main.rs".into(),
                text: "symbol is valid\n".into(),
                expected_revision: Some(saved),
            }))
            .await
            .unwrap();
        let Content::Diagnostics(items) = fixture.read(query, None).await.content else {
            panic!("diagnostics expected")
        };
        assert!(items.is_empty());
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if std::fs::read_to_string(fixture.root.join("messages"))
                    .unwrap()
                    .contains("textDocument/didSave")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCode {
                worktree: fixture.session.worktree,
                query: Query::Hover {
                    path: "../outside.rs".into(),
                    line: 1,
                    symbol: "name".into(),
                },
                overlay: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn remembers_plan_startup_and_preserves_denial() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("code", "code_hover"),
            json!({"path":"main.rs","line":1,"symbol":"symbol"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        fixture.config(WorkMode::Plan, Permission::Ask).await;
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(approval.startup.is_some());
        assert!(!approval.can_remember);
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!fixture.root.join("pid").exists());
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        let page = fixture.finish(turn).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result.to_string().contains("fixture signature"))
        );
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let next = fixture.submit().await;
        let page = fixture.finish(next).await;
        assert!(
            !page
                .approvals
                .iter()
                .any(|approval| approval.turn == next && approval.state == ApprovalState::Pending)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
            pid
        );
        fixture.settings(vec!["changed".into()], true).await;
        let denied = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        process::decide(
            &fixture.client,
            &approval,
            Decision::DenyWithReason("Keep the current service".into()),
        )
        .await;
        let page = fixture.finish(denied).await;
        assert!(
            results(&page)
                .iter()
                .any(|result| result.to_string().contains("Keep the current service"))
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
            pid
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn renews_startup_approval_after_executable_changes() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("code", "code_hover"),
            json!({"path":"main.rs","line":1,"symbol":"symbol"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        fixture.config(WorkMode::Code, Permission::Ask).await;
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        let original = approval.startup.clone().unwrap();
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        fixture.finish(turn).await;
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let warm = fixture.submit().await;
        let page = fixture.finish(warm).await;
        assert!(
            !page.approvals.iter().any(|approval| {
                approval.turn == warm && approval.state == ApprovalState::Pending
            })
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
            pid
        );
        let executable = fixture.bin.join("rust-analyzer");
        let mut bytes = std::fs::read(&executable).unwrap();
        bytes.extend_from_slice(b"\n# replacement\n");
        std::fs::write(executable, bytes).unwrap();
        let changed = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        let replacement = approval.startup.as_ref().unwrap();
        assert_eq!(approval.source, ApprovalSource::User);
        assert_eq!(replacement.executable, original.executable);
        assert_ne!(replacement.revision, original.revision);
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        fixture.finish(changed).await;
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("pid")).unwrap(),
            pid
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restarts_with_precise_durable_grants() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("code", "code_hover"),
            json!({"path":"main.rs","line":1,"symbol":"symbol"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        let turn = fixture.submit().await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        let descriptor = approval.startup.clone().unwrap();
        process::decide(&fixture.client, &approval, Decision::Approve).await;
        fixture.finish(turn).await;
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_with_environment(
            fixture.directory.path().join("node"),
            environment(fixture.directory.path(), &fixture.bin),
        )
        .await
        .unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        fixture.client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        fixture.node = node;
        let turn = fixture.submit().await;
        let page = fixture.finish(turn).await;
        let reused = page
            .approvals
            .iter()
            .find(|approval| approval.turn == turn)
            .unwrap();
        assert_eq!(reused.source, ApprovalSource::CodeRule);
        assert_eq!(reused.startup.as_ref(), Some(&descriptor));
        assert_eq!(reused.state, ApprovalState::Approved);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn starts_project_and_full_in_each_mode() {
    for remote in [false, true] {
        let server = Server::turn_tools(vec![(
            plugin_tool("code", "code_hover"),
            json!({"path":"main.rs","line":1,"symbol":"symbol"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        for (index, (permission, mode)) in [
            (Permission::Project, WorkMode::Plan),
            (Permission::Project, WorkMode::Code),
            (Permission::Full, WorkMode::Plan),
            (Permission::Full, WorkMode::Code),
        ]
        .into_iter()
        .enumerate()
        {
            fixture.config(mode, permission).await;
            fixture.settings(vec![format!("case-{index}")], true).await;
            let turn = fixture.submit().await;
            let page = fixture.finish(turn).await;
            let approval = page
                .approvals
                .iter()
                .find(|approval| approval.turn == turn)
                .unwrap();
            assert_eq!(approval.state, ApprovalState::Approved);
            assert_eq!(
                approval.source,
                if permission == Permission::Project {
                    ApprovalSource::Project
                } else {
                    ApprovalSource::Full
                }
            );
            assert!(
                results(&page)
                    .iter()
                    .any(|result| result.to_string().contains("fixture signature"))
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn attaches_edit_errors_only_when_enabled() {
    for remote in [false, true] {
        for enabled in [true, false] {
            let expected_revision = blake3::hash("🙂 broken symbol\n".as_bytes())
                .to_hex()
                .to_string();
            let server=Server::tools(vec![(plugin_tool("code","code_hover"),json!({"path":"main.rs","line":1,"symbol":"symbol"})),(plugin_tool("files","edit_file"),json!({"path":"main.rs","old_text":"broken","new_text":"broken_again","expected_revision":expected_revision}))]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            fixture.config(WorkMode::Code, Permission::Project).await;
            fixture.settings(Vec::new(), enabled).await;
            let turn = fixture.submit().await;
            let page = fixture.finish(turn).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn)
                    .unwrap()
                    .status,
                Status::Completed
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
                "🙂 broken_again symbol\n"
            );
            let result = results(&page)
                .into_iter()
                .find(|result| {
                    result.pointer("/data/path").and_then(Value::as_str) == Some("main.rs")
                })
                .unwrap();
            assert_eq!(
                result.pointer("/data/diagnostics_after_edit").is_some(),
                enabled
            );
            if enabled {
                assert!(result.to_string().contains("fixture syntax error"));
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn recovers_original_receipts_after_discovery_changes() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        let query = Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        };
        let startup = fixture.read(query, None).await.startup.unwrap();
        let request = fixture.client.prepare(Command::StartCodeServer { startup });
        let expected = fixture.client.execute(request.clone()).await.unwrap();
        let executable = fixture.bin.join("rust-analyzer");
        let mut bytes = std::fs::read(&executable).unwrap();
        bytes.extend_from_slice(b"\n# replacement\n");
        std::fs::write(executable, bytes).unwrap();
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            expected
        );
        let mut conflicting = request.clone();
        if let Command::StartCodeServer { startup } = &mut conflicting.command {
            startup.revision = "0".repeat(64);
        }
        assert_eq!(
            fixture.client.execute(conflicting).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        fixture.node.shutdown().await.unwrap();
        std::fs::remove_file(fixture.root.join("pid")).unwrap();
        let node = Node::start_with_environment(
            fixture.directory.path().join("node"),
            environment(fixture.directory.path(), &fixture.bin),
        )
        .await
        .unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        fixture.client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        fixture.node = node;
        assert_eq!(fixture.client.execute(request).await.unwrap(), expected);
        assert!(!fixture.root.join("pid").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_receipts_after_worktree_removal() {
    use std::io::Write;

    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let repository = git2::Repository::init(&fixture.root).unwrap();
        let exclude = repository.path().join("info/exclude");
        std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(exclude)
            .unwrap()
            .write_all(b"\n/messages\n/pid\n")
            .unwrap();
        let mut index = repository.index().unwrap();
        for name in ["Cargo.toml", "main.rs", "target.rs"] {
            index.add_path(std::path::Path::new(name)).unwrap();
        }
        index.write().unwrap();
        let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
            .unwrap()
            .to_string();
        let linked = fixture.directory.path().join("linked");
        let Output::Worktree(worktree) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateWorktree {
                project: fixture.session.project.unwrap(),
                path: linked.to_str().unwrap().into(),
                branch: "code-fixture".into(),
                commit: commit.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("worktree expected")
        };
        let Output::Code(result) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadCode {
                worktree: worktree.id,
                query: Query::Diagnostics {
                    path: "main.rs".into(),
                    severity: None,
                },
                overlay: None,
            }))
            .await
            .unwrap()
        else {
            panic!("code expected")
        };
        let request = fixture.client.prepare(Command::StartCodeServer {
            startup: result.startup.unwrap(),
        });
        let expected = fixture.client.execute(request.clone()).await.unwrap();
        assert!(linked.join("messages").is_file());
        assert!(linked.join("pid").is_file());
        let Output::GitStatus(status) = fixture
            .client
            .execute(fixture.client.prepare(Command::InspectGit {
                worktree: worktree.id,
            }))
            .await
            .unwrap()
        else {
            panic!("Git status expected")
        };
        assert!(!status.truncated);
        assert!(status.entries.is_empty(), "{status:?}");
        assert_eq!(status.head.as_deref(), Some(commit.as_str()));
        fixture
            .client
            .execute(fixture.client.prepare(Command::RemoveWorktree {
                worktree: worktree.id,
                expected_head: commit,
                expected_branch: "code-fixture".into(),
            }))
            .await
            .unwrap();
        assert!(!linked.exists());
        assert_eq!(fixture.client.execute(request).await.unwrap(), expected);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_diagnostics_in_the_checkpoint_worktree() {
    for remote in [false, true] {
        let expected_revision = blake3::hash("🙂 broken symbol\n".as_bytes())
            .to_hex()
            .to_string();
        let server=Server::tools(vec![(plugin_tool("code","code_hover"),json!({"path":"main.rs","line":1,"symbol":"symbol"})),(plugin_tool("files","edit_file"),json!({"path":"main.rs","old_text":"broken","new_text":"valid","expected_revision":expected_revision}))]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        let commit = {
            let repository = git2::Repository::init(&fixture.root).unwrap();
            let mut index = repository.index().unwrap();
            index
                .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
            let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
            let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
            repository
                .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
                .unwrap()
        };
        fixture.config(WorkMode::Code, Permission::Project).await;
        let turn = fixture.submit().await;
        fixture.finish(turn).await;
        let Output::FileCheckpoints(page) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListFileCheckpoints {
                session: fixture.session.id,
                turn,
                before: None,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("checkpoints expected")
        };
        assert_eq!(page.files.len(), 1);
        let checkpoint = &page.files[0];
        assert_eq!(checkpoint.worktree, fixture.session.worktree);
        let other = fixture.directory.path().join("other");
        let Output::Worktree(destination) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateWorktree {
                project: fixture.session.project.unwrap(),
                path: other.to_str().unwrap().into(),
                branch: "other".into(),
                commit: commit.to_string(),
            }))
            .await
            .unwrap()
        else {
            panic!("worktree expected")
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadSession {
                session: fixture.session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::MoveConversation {
                session: session.id,
                worktree: destination.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap();
        let before = std::fs::read_to_string(fixture.root.join("messages"))
            .unwrap()
            .len();
        let Output::FileRestored(restored) = fixture
            .client
            .execute(fixture.client.prepare(Command::RestoreFileCheckpoint {
                session: session.id,
                checkpoint: checkpoint.id,
                worktree: checkpoint.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("restore expected")
        };
        assert_eq!(restored.path, "main.rs");
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let messages = std::fs::read_to_string(fixture.root.join("messages")).unwrap();
                let changed = messages[before..]
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .collect::<Vec<_>>();
                if changed.iter().any(|message| {
                    message["method"] == "textDocument/didChange"
                        && message
                            .pointer("/params/contentChanges/0/text")
                            .and_then(Value::as_str)
                            == Some("🙂 broken symbol\n")
                }) && changed
                    .iter()
                    .any(|message| message["method"] == "textDocument/didSave")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(other.join("main.rs")).unwrap(),
            "🙂 broken symbol\n"
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
