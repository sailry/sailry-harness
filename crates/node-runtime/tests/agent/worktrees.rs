use super::*;
use serde_json::{Value, json};
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
    let node = Node::start(directory.join("node")).await.unwrap();
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

fn results(page: &Page) -> Vec<Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect()
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
        init(&root);
        let (node, controller, client) = connect(dir.path(), remote).await;
        let bootstrap = Server::tools(vec![]).await;
        let (session, provider) = configured(&client, &bootstrap.endpoint, &root).await;
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
        let values = results(&page);
        assert_eq!(values.len(), 3);
        assert!(values[0].get("error").is_none(), "{values:?}");
        let tree: Worktree = serde_json::from_value(values[0]["data"].clone()).unwrap();
        assert_ne!(tree.id, session.worktree);
        assert_eq!(tree.project, session.project);
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("source.txt")).unwrap(),
            "pending\n"
        );
        assert_eq!(values[2]["data"]["text"], "pending\n");
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
        let (session, mut provider) = configured(&client, &bootstrap.endpoint, &root).await;
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
        let values = results(&page);
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
        let child = Server::tools(vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path":"child.txt","text":"isolated child","expected_revision":null}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path":"child.txt"}),
            ),
        ])
        .await;
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
        let parent = Server::tools(vec![
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
        ])
        .await;
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
        let values = results(&page);
        assert_eq!(values[0]["status"], "completed", "{values:?}");
        assert_eq!(values[3]["error"]["code"], "conflict");
        let child_id: SessionId = serde_json::from_value(values[0]["session"].clone()).unwrap();
        let child_page = history(&client, child_id).await;
        assert_eq!(
            results(&child_page)[1]["data"]["text"],
            "isolated child",
            "{:?}",
            results(&child_page)
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
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        let mut projection = sailry_client::conversation::Projection::new(node.id(), session.id, 1);
        projection.apply(1, updates.next().await.unwrap()).unwrap();
        assert_eq!(
            projection.snapshot().unwrap().page.children[0].run.worktree,
            tree.id
        );
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start(dir.path().join("node")).await.unwrap();
        let restored = history(&Client::new(node.local()), session.id).await;
        assert_eq!(restored.children[0].run.worktree, tree.id);
        assert_eq!(child.requests.lock().unwrap().len(), 3);
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
        let mut calls = Vec::new();
        let mut targets = Vec::new();
        for index in 0..2 {
            let Output::Worktree(tree) = execute(
                &client,
                Command::CreateWorktree {
                    project: session.project.unwrap(),
                    path: dir
                        .path()
                        .join(format!("child-{index}"))
                        .to_str()
                        .unwrap()
                        .into(),
                    branch: format!("child-{index}"),
                    commit: commit.clone(),
                },
            )
            .await
            else {
                panic!("worktree expected")
            };
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
        let values = results(&page);
        assert_eq!(values.len(), 2);
        assert!(
            values.iter().all(|value| value["status"] == "completed"),
            "{values:?}"
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
