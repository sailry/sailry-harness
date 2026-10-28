use super::*;
use serde_json::json;
use std::path::Path;

#[path = "worktrees/live.rs"]
mod live;

fn init(root: &Path) -> String {
    std::fs::create_dir_all(root).unwrap();
    let repo = git2::Repository::init(root).unwrap();
    std::fs::write(root.join("source.txt"), "original\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("source.txt")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &author, &author, "Initial", &tree, &[])
        .unwrap()
        .to_string()
}

async fn connect(directory: &Path, remote: bool) -> (Node, Link, Client) {
    let node = Node::start_empty(directory.join("node")).await.unwrap();
    let controller = Link::controller(directory.join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let peer = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(if remote {
        controller.handle().remote(peer)
    } else {
        node.local()
    });
    (node, controller, client)
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn enable(client: &Client, mut session: Session, mut provider: Provider) -> Session {
    provider.models = vec![super::delegation::model("fixture-a")];
    provider.default_model = "fixture-a".into();
    execute(
        client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await;
    session.config.permission = Permission::Full;
    let Output::Session(session) = execute(
        client,
        Command::SetSessionConfig {
            session: session.id,
            expected_revision: session.revision,
            config: session.config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

async fn run(client: &Client, session: &Session) -> Page {
    let Output::QueuedTurn(turn) = execute(
        client,
        Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Manage isolated work".into(),
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    let page = finished(client, session.id, turn.id).await;
    assert_eq!(
        page.runs[0].status,
        Status::Completed,
        "{:?}",
        page.runs[0].error
    );
    page
}

#[tokio::test]
async fn creates_registered_checkout() {
    for remote in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let commit = init(&root);
        let ignore = b"ignored.txt\n";
        let exclude = b"# User exclusions\nprivate/";
        std::fs::write(root.join(".gitignore"), ignore).unwrap();
        std::fs::write(root.join(".git/info/exclude"), exclude).unwrap();
        let retained = root.join(".sailry/worktrees/retained");
        std::fs::create_dir_all(&retained).unwrap();
        std::fs::write(retained.join("marker.txt"), "retained").unwrap();
        let (node, controller, client) = connect(dir.path(), remote).await;
        let bootstrap = Server::tools(vec![]).await;
        approvals::install(
            &client,
            &["worktrees", "files", "commands", "git", "delegation"],
        )
        .await;
        let (session, provider) = configured_session(&client, &bootstrap.endpoint, &root).await;
        let Output::GitStatus(status) = execute(
            &client,
            Command::InspectGit {
                worktree: session.worktree,
            },
        )
        .await
        else {
            panic!("status expected")
        };
        std::fs::write(root.join("source.txt"), "pending\n").unwrap();
        let server = Server::tools(vec![
            (plugin_tool("worktrees", "create_worktree"), json!({"branch":"agent-task", "expected_head":status.head, "expected_index":status.index_revision, "include_changes":true})),
            (plugin_tool("worktrees", "list_worktrees"), json!({})),
            (plugin_tool("files", "read_file"), json!({"path":"source.txt"})),
        ]).await;
        let mut provider = provider;
        provider.endpoint = server.endpoint.clone();
        let session = enable(&client, session, provider).await;
        let page = run(&client, &session).await;
        let values = results(&client, &page).await;
        assert_eq!(values.len(), 3);
        assert!(values[0].get("error").is_none(), "{values:?}");
        let tree: Worktree = serde_json::from_value(values[0]["data"].clone()).unwrap();
        assert_ne!(tree.id, session.worktree);
        assert_eq!(tree.project, session.project);
        let destination = root.canonicalize().unwrap().join(".sailry/worktrees");
        assert_eq!(Path::new(&tree.path), destination.join(tree.id.to_string()));
        assert!(!Path::new(&tree.path).join(".sailry").exists());
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("source.txt")).unwrap(),
            "pending\n"
        );
        assert_eq!(values[2]["data"]["lines"][0]["text"], "pending\n");
        assert!(
            values[1]["data"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == json!(tree.id))
        );
        assert_eq!(page.runs[0].worktree, session.worktree);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Full);
        std::fs::write(
            Path::new(&tree.path).join("child-source.txt"),
            "child snapshot",
        )
        .unwrap();
        let Output::GitStatus(child_status) =
            execute(&client, Command::InspectGit { worktree: tree.id }).await
        else {
            panic!("status expected")
        };
        let Output::Worktree(sibling) = execute(
            &client,
            Command::CreateManagedWorktree {
                project: session.project.unwrap(),
                source: tree.id,
                branch: "agent-next".into(),
                expected_head: commit,
                expected_index: child_status.index_revision.unwrap(),
                include_changes: true,
            },
        )
        .await
        else {
            panic!("worktree expected")
        };
        assert_eq!(
            Path::new(&sibling.path),
            destination.join(sibling.id.to_string())
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&sibling.path).join("child-source.txt")).unwrap(),
            "child snapshot"
        );
        assert!(!root.join("child-source.txt").exists());
        assert!(!Path::new(&tree.path).join(".sailry").exists());
        assert!(!Path::new(&sibling.path).join(".sailry").exists());
        assert_eq!(std::fs::read(root.join(".gitignore")).unwrap(), ignore);
        let exclusions = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
        assert_eq!(exclusions, "# User exclusions\nprivate/\n.sailry/\n");
        assert_eq!(
            exclusions
                .lines()
                .filter(|line| *line == ".sailry/")
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(retained.join("marker.txt")).unwrap(),
            "retained"
        );
        let Output::GitStatus(status) = execute(
            &client,
            Command::InspectGit {
                worktree: session.worktree,
            },
        )
        .await
        else {
            panic!("status expected")
        };
        assert!(
            status
                .entries
                .iter()
                .all(|entry| entry.path != ".sailry" && !entry.path.starts_with(".sailry/")),
            "{status:?}"
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn scopes_reads_and_removal() {
    for remote in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let commit = init(&root);
        let (node, controller, client) = connect(dir.path(), remote).await;
        let bootstrap = Server::tools(vec![]).await;
        approvals::install(&client, &["git", "worktrees"]).await;
        let (session, mut provider) = configured_session(&client, &bootstrap.endpoint, &root).await;
        let Output::Worktree(tree) = execute(
            &client,
            Command::CreateWorktree {
                project: session.project.unwrap(),
                path: dir.path().join("linked").to_str().unwrap().into(),
                branch: "linked".into(),
                commit: commit.clone(),
            },
        )
        .await
        else {
            panic!("worktree expected")
        };
        let foreign = dir.path().join("foreign");
        init(&foreign);
        let Output::Project(other) = execute(
            &client,
            Command::RegisterProject {
                name: "Other".into(),
                path: foreign.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let foreign_id = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(other.id))
            .unwrap()
            .id;
        let server = Server::tools(vec![
            (
                plugin_tool("git", "git_status"),
                json!({"worktree":tree.id}),
            ),
            (plugin_tool("git", "git_log"), json!({"worktree":tree.id})),
            (
                plugin_tool("git", "git_diff"),
                json!({"worktree":tree.id,"path":"source.txt"}),
            ),
            (
                plugin_tool("git", "git_status"),
                json!({"worktree":foreign_id}),
            ),
            (
                plugin_tool("worktrees", "remove_worktree"),
                json!({"worktree":foreign_id,"expected_head":commit,"expected_branch":"linked"}),
            ),
            (
                plugin_tool("worktrees", "register_worktree"),
                json!({"path":tree.path}),
            ),
            (
                plugin_tool("worktrees", "remove_worktree"),
                json!({"worktree":tree.id,"expected_head":commit,"expected_branch":"linked"}),
            ),
        ])
        .await;
        provider.endpoint = server.endpoint.clone();
        let session = enable(&client, session, provider).await;
        let page = run(&client, &session).await;
        let values = results(&client, &page).await;
        assert_eq!(values.len(), 7, "{values:?}");
        for i in [0, 1, 2, 5, 6] {
            assert!(values[i].get("error").is_none(), "{values:?}");
        }
        for i in [3, 4] {
            assert_eq!(values[i]["error"]["code"], "permission_denied");
        }
        assert_eq!(values[0]["data"]["branch"], "linked");
        assert_eq!(values[5]["data"]["id"], json!(tree.id));
        assert!(!Path::new(&tree.path).exists());
        assert!(root.join("source.txt").exists());
        assert!(foreign.join("source.txt").exists());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_child_writes() {
    for remote in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let commit = init(&root);
        let (node, controller, client) = connect(dir.path(), remote).await;
        let bootstrap = Server::tools(vec![]).await;
        let mut child_calls = vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path":"child.txt","text":"isolated child","expected_revision":null}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path":"child.txt"}),
            ),
        ];
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            child_calls.push((
                plugin_tool("commands", "run_command"),
                json!({"command":"printf command-child > command-child.txt || exit 90; printf child-command"}),
            ));
        }
        let child = Server::tools(child_calls).await;
        let (session, _, _) = super::delegation::setup(&client, &root, &bootstrap, &child).await;
        let Output::GitStatus(status) = execute(
            &client,
            Command::InspectGit {
                worktree: session.worktree,
            },
        )
        .await
        else {
            panic!("Git status expected")
        };
        let Output::Worktree(tree) = execute(
            &client,
            Command::CreateManagedWorktree {
                project: session.project.unwrap(),
                source: session.worktree,
                branch: "child".into(),
                expected_head: commit.clone(),
                expected_index: status.index_revision.unwrap(),
                include_changes: false,
            },
        )
        .await
        else {
            panic!("worktree expected")
        };
        let mut parent_calls = vec![
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role":"review","task":"Write the child file", "worktree":tree.id}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path":"parent.txt","text":"isolated parent","expected_revision":null}),
            ),
            (
                plugin_tool("git", "git_status"),
                json!({"worktree":tree.id}),
            ),
            (
                plugin_tool("worktrees", "remove_worktree"),
                json!({"worktree":tree.id,"expected_head":commit,"expected_branch":"child"}),
            ),
        ];
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let rename = if cfg!(target_os = "macos") {
                "if mv .sailry moved-metadata; then exit 95; fi".into()
            } else {
                format!(
                    "if mv .sailry moved-metadata; then \
                         if printf crossed > moved-metadata/worktrees/{}/child.txt; then exit 95; fi; \
                         mv moved-metadata .sailry || exit 96; fi",
                    tree.id
                )
            };
            parent_calls.push((
                plugin_tool("commands", "run_command"),
                json!({"command":format!(
                    "printf command-parent > command-parent.txt || exit 90; \
                     printf note > .sailry/notes.txt || exit 91; \
                     if printf crossed > .sailry/worktrees/{0}/child.txt; then exit 92; fi; \
                     if printf crossed > .sailry/worktrees/{0}/crossed.txt; then exit 93; fi; \
                     if mkdir .sailry/worktrees/parent-created; then exit 94; fi; {1}; printf protected",
                    tree.id, rename
                )}),
            ));
        }
        let parent = Server::tools(parent_calls).await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == session.config.provider)
            .unwrap()
            .clone();
        provider.endpoint = parent.endpoint.clone();
        let session = enable(&client, session, provider).await;
        let page = run(&client, &session).await;
        let values = results(&client, &page).await;
        super::delegation::completed(&client, &page, &parent, &values[0], "answer-fixture-a").await;
        assert_eq!(values[3]["error"]["code"], "conflict");
        let child_id: SessionId = serde_json::from_value(values[0]["session"].clone()).unwrap();
        let child_page = history(&client, child_id).await;
        assert_eq!(
            results(&client, &child_page).await[1]["data"]["lines"][0]["text"],
            "isolated child",
            "{:?}",
            results(&client, &child_page).await
        );
        assert_eq!(child_page.runs[0].worktree, tree.id);
        assert_eq!(page.children[0].run.worktree, tree.id);
        assert_eq!(page.runs[0].worktree, session.worktree);
        assert!(!root.join("child.txt").exists());
        assert!(!Path::new(&tree.path).join("parent.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("parent.txt")).unwrap(),
            "isolated parent"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("child.txt")).unwrap(),
            "isolated child"
        );
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use sailry_protocol::process::{Access, Outcome};
            for (page, text) in [(&page, "protected"), (&child_page, "child-command")] {
                let outputs = super::process::results(&client, page).await;
                assert_eq!(outputs.len(), 1, "{page:?}");
                assert_eq!(outputs[0].outcome, Outcome::Exited(0), "{outputs:?}");
                assert_eq!(outputs[0].stdout.text, text);
                let approvals: Vec<_> = page
                    .approvals
                    .iter()
                    .filter(|approval| approval.access == Some(Access::Workspace))
                    .collect();
                assert_eq!(approvals.len(), 1, "{page:?}");
                assert_eq!(approvals[0].source, ApprovalSource::Sandbox);
                assert_eq!(approvals[0].state, ApprovalState::Approved);
            }
            assert_eq!(
                std::fs::read_to_string(root.join("command-parent.txt")).unwrap(),
                "command-parent"
            );
            assert_eq!(
                std::fs::read_to_string(root.join(".sailry/notes.txt")).unwrap(),
                "note"
            );
            assert_eq!(
                std::fs::read_to_string(Path::new(&tree.path).join("command-child.txt")).unwrap(),
                "command-child"
            );
            assert!(!root.join("command-child.txt").exists());
            assert!(!Path::new(&tree.path).join("command-parent.txt").exists());
            assert!(!Path::new(&tree.path).join("crossed.txt").exists());
            assert!(!root.join(".sailry/worktrees/parent-created").exists());
            assert!(!root.join("moved-metadata").exists());
        }
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        let mut projection = sailry_client::conversation::Projection::new(node.id(), session.id, 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        assert_eq!(
            projection.snapshot().unwrap().page.children[0].run.worktree,
            tree.id
        );
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(dir.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let restored = history(&client, session.id).await;
        assert_eq!(restored, page);
        assert_eq!(restored.children[0].run.worktree, tree.id);
        assert_eq!(
            child.requests.lock().unwrap().len(),
            if cfg!(any(target_os = "macos", target_os = "linux")) {
                4
            } else {
                3
            }
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .projects
            .into_iter()
            .find(|project| Some(project.id) == session.project)
            .unwrap();
        assert_eq!(
            execute(
                &client,
                Command::RemoveProject {
                    expected: project.clone()
                }
            )
            .await,
            Output::ProjectRemoved { id: project.id }
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(snapshot.projects.iter().all(|entry| entry.id != project.id));
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("child.txt")).unwrap(),
            "isolated child"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("parent.txt")).unwrap(),
            "isolated parent"
        );
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("command-child.txt")).unwrap(),
            "command-child"
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn runs_parallel_checkouts() {
    for remote in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let commit = init(&root);
        let (node, controller, client) = connect(dir.path(), remote).await;
        let bootstrap = Server::tools(vec![]).await;
        let child = Server::gate(2).await;
        let (session, _, _) = super::delegation::setup(&client, &root, &bootstrap, &child).await;
        let Output::GitStatus(status) = execute(
            &client,
            Command::InspectGit {
                worktree: session.worktree,
            },
        )
        .await
        else {
            panic!("Git status expected")
        };
        let mut calls = Vec::new();
        let mut targets = Vec::new();
        for index in 0..2 {
            let Output::Worktree(tree) = execute(
                &client,
                Command::CreateManagedWorktree {
                    project: session.project.unwrap(),
                    source: session.worktree,
                    branch: format!("child-{index}"),
                    expected_head: commit.clone(),
                    expected_index: status.index_revision.clone().unwrap(),
                    include_changes: false,
                },
            )
            .await
            else {
                panic!("worktree expected")
            };
            assert_eq!(
                Path::new(&tree.path),
                root.canonicalize()
                    .unwrap()
                    .join(".sailry/worktrees")
                    .join(tree.id.to_string())
            );
            targets.push(tree.id);
            calls.push((
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role":"review","task":format!("Inspect task {index}"),"worktree":tree.id}),
            ));
        }
        let parent = Server::parallel(calls).await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let mut provider = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == session.config.provider)
            .unwrap()
            .clone();
        provider.endpoint = parent.endpoint.clone();
        let session = enable(&client, session, provider).await;
        let page = run(&client, &session).await;
        let values: Vec<_> = results(&client, &page)
            .await
            .into_iter()
            .filter(|value| value["admitted"] == true)
            .collect();
        assert_eq!(values.len(), 2);
        for receipt in &values {
            super::delegation::completed(&client, &page, &parent, receipt, "answer-fixture-a")
                .await;
        }
        assert_eq!(
            tool_details(&client, &page)
                .await
                .iter()
                .filter(|(name, _)| name == "tool_search")
                .count(),
            1
        );
        assert_eq!(child.requests.lock().unwrap().len(), 2);
        assert_eq!(page.children.len(), 2);
        for target in targets {
            assert!(
                page.children
                    .iter()
                    .any(|child| child.run.worktree == target)
            );
        }
        assert_eq!(page.runs[0].worktree, session.worktree);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
