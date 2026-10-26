#![cfg(unix)]
use super::*;
use sailry_protocol::conversation::reference;
use serde_json::{Value, json};

const SOURCE: &str = "pub fn observed_symbol() -> usize {\n    7\n}\n";

async fn execute(fixture: &process::Fixture, command: Command) -> Output {
    fixture
        .client
        .execute(fixture.client.prepare(command))
        .await
        .unwrap()
}

async fn unassigned(fixture: &mut process::Fixture) {
    let Output::Session(session) = execute(
        fixture,
        Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(fixture.session.config.clone()),
        },
    )
    .await
    else {
        panic!("session expected");
    };
    let Output::Snapshot(snapshot) = execute(fixture, Command::Snapshot).await else {
        panic!("snapshot expected");
    };
    let worktree = snapshot
        .worktrees
        .iter()
        .find(|worktree| worktree.id == session.worktree)
        .unwrap();
    assert_eq!(worktree.project, None);
    assert_ne!(session.worktree, fixture.session.worktree);
    fixture.root = worktree.path.clone().into();
    fixture.session = session;
}

fn message(package: &str) -> Input {
    Input {
        text: "Find the requested source context".into(),
        attachments: vec![],
        references: vec![reference::Reference {
            target: reference::Target::Plugin(package.into()),
            label: package.into(),
        }],
    }
}

async fn submit(fixture: &process::Fixture, package: &str) -> QueuedTurn {
    let Output::QueuedTurn(turn) = execute(
        fixture,
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: message(package),
        },
    )
    .await
    else {
        panic!("turn expected");
    };
    turn
}

fn schemas(server: &Server) {
    let requests = server.requests.lock().unwrap();
    let tools = requests.first().unwrap()["tools"].as_array().unwrap();
    for name in ["code_search", "code_outline", "code_context"] {
        let alias = plugin_tool("code", name);
        let schema = tools
            .iter()
            .find(|tool| tool["function"]["name"] == alias)
            .unwrap_or_else(|| panic!("missing declared tool {name}"));
        assert_eq!(schema["function"]["parameters"]["type"], "object");
    }
}

async fn result(client: &Client, page: &Page, name: &str) -> Value {
    let alias = plugin_tool("code", name);
    let value = tool_details(client, page)
        .await
        .into_iter()
        .find_map(|(name, detail)| (name == alias).then_some(detail.result.unwrap()))
        .unwrap_or_else(|| panic!("missing result for {name}"));
    assert!(value.get("error").is_none(), "{value:?}");
    value
}

async fn capture(fixture: &process::Fixture, turn: &QueuedTurn) {
    let Output::QueuedTurn(captured) = execute(
        fixture,
        Command::ReadTurnCapture {
            session: fixture.session.id,
            turn: turn.id,
        },
    )
    .await
    else {
        panic!("turn capture expected");
    };
    assert_eq!(captured.plugins, turn.plugins);
    let reference = captured
        .plugins
        .iter()
        .find(|reference| reference.name == "code")
        .unwrap();
    let Output::Plugin(info) = execute(
        fixture,
        Command::ReadPluginVersion {
            package: reference.clone(),
        },
    )
    .await
    else {
        panic!("captured package expected");
    };
    assert_eq!(info.summary.digest, reference.digest);
    assert_eq!(info.summary.version.as_deref(), Some("0.1.10"));
    assert_eq!(
        info.extension
            .unwrap()
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["code_search", "code_outline", "code_context"]
    );
}

#[tokio::test]
async fn executes_in_existing_worktrees() {
    for remote in [false, true] {
        for assigned in [false, true] {
            let server = Server::tools(vec![
                (
                    plugin_tool("code", "code_search"),
                    json!({"name":"observed_symbol"}),
                ),
                (
                    plugin_tool("code", "code_outline"),
                    json!({"path":"source.rs"}),
                ),
                (
                    plugin_tool("code", "code_context"),
                    json!({"name":"observed_symbol"}),
                ),
            ])
            .await;
            let mut fixture = process::Fixture::packages(remote, &server, &["code"]).await;
            if !assigned {
                unassigned(&mut fixture).await;
            }
            execute(
                &fixture,
                Command::WriteFile {
                    worktree: fixture.session.worktree,
                    path: "source.rs".into(),
                    text: SOURCE.into(),
                    expected_revision: None,
                },
            )
            .await;
            let turn = submit(&fixture, "code").await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert!(page.approvals.is_empty(), "{page:?}");
            schemas(&server);
            capture(&fixture, &turn).await;

            let revision = blake3::hash(SOURCE.as_bytes()).to_hex().to_string();
            for name in ["code_search", "code_outline", "code_context"] {
                let output = result(&fixture.client, &page, name).await;
                assert!(
                    output["symbols"].as_array().unwrap().iter().any(|symbol| {
                        symbol["name"] == "observed_symbol"
                            && symbol["path"] == "source.rs"
                            && symbol["file_revision"] == revision
                    }),
                    "{output:?}"
                );
                assert!(output["index_revision"].as_u64().unwrap() > 0);
                assert_eq!(output["partial"], false, "{output:?}");
                assert_eq!(output["truncated"], false, "{output:?}");
            }
            let outline = result(&fixture.client, &page, "code_outline").await;
            assert_eq!(outline["status"], "parsed");
            assert_eq!(outline["file_revision"], revision);
            let context = result(&fixture.client, &page, "code_context").await;
            let snippets = context["snippets"].as_array().unwrap();
            assert_eq!(snippets.len(), 1, "{context:?}");
            assert_eq!(snippets[0]["file_revision"], revision);
            assert_eq!(snippets[0]["text"], SOURCE);
            assert!(serde_json::to_vec(&context).unwrap().len() <= 16 * 1024);
            assert!(
                fixture
                    .node
                    .profile()
                    .join("cache/code-index")
                    .join(fixture.session.worktree.to_string())
                    .join("index.turso")
                    .is_file()
            );
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn searches_empty_conversation_worktrees() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (plugin_tool("code", "code_search"), json!({})),
            (plugin_tool("code", "code_context"), json!({})),
        ])
        .await;
        let mut fixture = process::Fixture::packages(remote, &server, &["code"]).await;
        unassigned(&mut fixture).await;
        let turn = submit(&fixture, "code").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{page:?}"
        );
        schemas(&server);
        for name in ["code_search", "code_context"] {
            let output = result(&fixture.client, &page, name).await;
            assert!(
                output["symbols"].as_array().unwrap().is_empty(),
                "{output:?}"
            );
            assert_eq!(output["partial"], false, "{output:?}");
            assert_eq!(output["truncated"], false, "{output:?}");
        }
        assert!(
            result(&fixture.client, &page, "code_context").await["snippets"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
