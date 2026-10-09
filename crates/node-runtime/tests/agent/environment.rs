#![cfg(unix)]
use super::*;
use serde_json::json;
#[path = "../support/environment.rs"]
mod fixture;

impl fixture::Login {
    fn captures(&self) -> usize {
        std::fs::read(self.home.join("captures"))
            .unwrap_or_default()
            .len()
    }
}

async fn connect(node: &Node, controller: &Link, remote: bool) -> Client {
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    Client::new(if remote {
        controller.handle().remote(address)
    } else {
        node.local()
    })
}

async fn run(client: &Client, session: &Session) -> Page {
    let turn = approvals::submit(client, session.id).await;
    for _ in 0..2 {
        let (_, approval) = approvals::pending(client, session.id).await;
        process::decide(client, &approval, Decision::Approve).await;
    }
    let mut page = finished(client, session.id, turn).await;
    page.entries.retain(|entry| entry.turn == turn);
    page.runs.retain(|run| run.turn == turn);
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    page
}

async fn background(
    client: &Client,
    session: SessionId,
    turn: TurnId,
) -> sailry_protocol::process::Snapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Output::Commands(commands) = client
                .execute(client.prepare(Command::ListCommands { session }))
                .await
                .unwrap()
            else {
                panic!("commands expected")
            };
            let command = commands
                .iter()
                .find(|command| command.turn == turn)
                .unwrap();
            let Output::CommandOutput(snapshot) = client
                .execute(client.prepare(Command::ReadCommand {
                    session,
                    id: command.id,
                }))
                .await
                .unwrap()
            else {
                panic!("command output expected")
            };
            if matches!(
                snapshot.info.status,
                sailry_protocol::process::Status::Finished(_)
            ) {
                return snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background command deadline")
}

#[tokio::test]
async fn freezes_login_until_restart() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let login = fixture::Login::new(directory.path());
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let mut node = Node::start_with_environment(&profile, login.source())
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let server = Server::turn_tools(vec![
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":"environment-fixture"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"access":"unrestricted","command":"environment-fixture", "background":true}),
            ),
        ])
        .await;
        let mut client = connect(&node, &controller, remote).await;
        let session = approvals::prepare(&client, &server, &root).await;
        isolate_model_tools(&client).await;
        assert_eq!(login.captures(), 1);
        for second in [false, true] {
            login.select(second);
            let page = run(&client, &session).await;
            let foreground = process::results(&page);
            assert_eq!(foreground.len(), 1);
            assert_eq!(
                foreground[0].outcome,
                sailry_protocol::process::Outcome::Exited(0)
            );
            assert_eq!(foreground[0].stdout.text, "first:unset:unset");
            assert_eq!(
                background(&client, session.id, page.runs.last().unwrap().turn)
                    .await
                    .stdout
                    .text,
                "first:unset:unset"
            );
            assert_eq!(login.captures(), 1);
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let public = serde_json::to_string(&snapshot).unwrap();
            assert!(!public.contains("login-secret"));
            assert!(!public.contains("ambient-model-secret"));
        }
        drop(client);
        let id = node.id();
        node.shutdown().await.unwrap();
        controller.handle().disconnect(id).await;
        node = Node::start_with_environment(&profile, login.source())
            .await
            .unwrap();
        client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            Client::new(node.local())
        };
        let page = run(&client, &session).await;
        assert_eq!(process::results(&page)[0].stdout.text, "second:unset:unset");
        assert_eq!(
            background(&client, session.id, page.runs.last().unwrap().turn)
                .await
                .stdout
                .text,
            "second:unset:unset"
        );
        assert_eq!(login.captures(), 2);
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shares_login_with_stdio() {
    use plugin::{mcp::Definition, settings::SecretUpdate};
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let login = fixture::Login::new(directory.path());
        for (programs, label) in [(&login.first, "first"), (&login.second, "second")] {
            fixture::executable(
                &programs.join("environment-peer"),
                &format!(
                    "#!/bin/sh\nprintf '{label}' >> \"$HOME/mcp-launches\"\nexec {} \"$@\"\n",
                    fixture::quote(std::env::current_exe().unwrap().to_str().unwrap())
                ),
            );
        }
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_with_environment(directory.path().join("node"), login.source())
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let client = connect(&node, &controller, remote).await;
        let server = Server::turn_tools(vec![(
            mcp::peer::package_alias("mcp-environment-peer", "server", "read"),
            json!({}),
        )])
        .await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut config = mcp::peer::config("mcp::peer::stdio_peer", "settings");
        config["env"].as_object_mut().unwrap().remove("PATH");
        config["env"]["SAILRY_MCP_TOKEN"] = json!("explicit-token");
        let mut definition = Definition::Stdio {
            command: "environment-peer".into(),
            args: serde_json::from_value(config["args"].clone()).unwrap(),
            env: config["env"].as_object().unwrap().keys().cloned().collect(),
        };
        let secrets = config["env"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    SecretUpdate::Replace(Secret::new(value.as_str().unwrap().into())),
                )
            })
            .collect();
        let Output::Plugin(installed) = client
            .execute(client.prepare(Command::InstallMcp {
                name: "mcp-environment-peer".into(),
                expected_revision: 0,
                definition: definition.clone(),
                secrets,
            }))
            .await
            .unwrap()
        else {
            panic!("MCP plugin expected")
        };
        let Output::Plugins(packages) = client
            .execute(client.prepare(Command::ListPlugins))
            .await
            .unwrap()
        else {
            panic!("plugins expected")
        };
        for package in packages
            .into_iter()
            .filter(|package| package.enabled && package.name != "mcp-environment-peer")
        {
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: package.name,
                    expected_revision: package.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
        login.select(true);
        let turn = approvals::submit(&client, session.id).await;
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0]["output"]["secret_hash"],
            blake3::hash(b"explicit-token").to_hex().to_string()
        );
        assert_eq!(login.captures(), 1);
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("login-token")
        );
        assert_eq!(
            std::fs::read_to_string(login.home.join("mcp-launches")).unwrap(),
            "first"
        );
        let Definition::Stdio { env, .. } = &mut definition else {
            panic!("stdio definition expected")
        };
        env.push("PATH".into());
        let mut secrets: std::collections::BTreeMap<_, _> = config["env"]
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), SecretUpdate::Keep))
            .collect();
        secrets.insert(
            "PATH".into(),
            SecretUpdate::Replace(Secret::new(login.second.to_str().unwrap().into())),
        );
        client
            .execute(client.prepare(Command::InstallMcp {
                name: "mcp-environment-peer".into(),
                expected_revision: installed.summary.revision,
                definition,
                secrets,
            }))
            .await
            .unwrap();
        let turn = approvals::submit(&client, session.id).await;
        let page = finished(&client, session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            std::fs::read_to_string(login.home.join("mcp-launches")).unwrap(),
            "firstsecond"
        );
        let result = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| &entry.parts)
            .find_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            result["output"]["secret_hash"],
            blake3::hash(b"explicit-token").to_hex().to_string()
        );
        assert_eq!(login.captures(), 1);
        node.shutdown().await.unwrap();
    }
}
