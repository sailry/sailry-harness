use super::*;
use std::collections::BTreeSet;
#[path = "plugins/assistants.rs"]
mod assistants;
#[path = "plugins/commands.rs"]
mod commands;
#[path = "plugins/failures.rs"]
mod failures;
#[path = "plugins/files.rs"]
mod files;
#[path = "plugins/flows.rs"]
mod flows;
#[path = "plugins/goal_history.rs"]
mod goal_history;
#[path = "plugins/http.rs"]
mod http;
#[path = "plugins/operations.rs"]
mod operations;
#[path = "plugins/reminders.rs"]
mod reminders;
#[path = "plugins/scheduled_tasks.rs"]
mod scheduled_tasks;

#[tokio::test]
async fn scopes_tool_switches_to_the_execution_node() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let other = Node::start(directory.path().join("other")).await.unwrap();
        let address = other
            .link()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            other.link().remote(address)
        } else {
            node.local()
        });
        let origin = Client::new(other.local());
        let server = Server::tools(vec![]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut plugins: Vec<(String, Vec<String>)> = Vec::new();
        for package in [
            "git",
            "commands",
            "files",
            "memory",
            "progress",
            "delegation",
            "worktrees",
        ] {
            let Output::Plugin(commands) = client
                .execute(client.prepare(Command::ReadPlugin {
                    name: package.into(),
                }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            plugins.push((
                commands.summary.name.clone(),
                commands
                    .extension
                    .unwrap()
                    .tools
                    .into_iter()
                    .map(|tool| plugin_tool(&commands.summary.name, &tool.name))
                    .collect(),
            ));
        }
        for (name, _) in &plugins {
            let Output::Plugin(info) = origin
                .execute(origin.prepare(Command::ReadPlugin { name: name.clone() }))
                .await
                .unwrap()
            else {
                panic!("plugin expected")
            };
            origin
                .execute(origin.prepare(Command::SetPluginEnabled {
                    name: name.clone(),
                    expected_revision: info.summary.revision,
                    enabled: false,
                }))
                .await
                .unwrap();
        }
        for enabled in [true, false, true] {
            if !server.requests.lock().unwrap().is_empty() {
                for (name, _) in &plugins {
                    let Output::Plugin(info) = client
                        .execute(client.prepare(Command::ReadPlugin { name: name.clone() }))
                        .await
                        .unwrap()
                    else {
                        panic!("plugin expected")
                    };
                    client
                        .execute(client.prepare(Command::SetPluginEnabled {
                            name: name.clone(),
                            expected_revision: info.summary.revision,
                            enabled,
                        }))
                        .await
                        .unwrap();
                }
            }
            let turn = approvals::submit(&client, session.id).await;
            let page = finished(&client, session.id, turn).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let requests = server.requests.lock().unwrap();
            let tools: BTreeSet<_> = requests.last().unwrap()["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            for (_, declarations) in &plugins {
                for name in declarations {
                    assert_eq!(tools.contains(name.as_str()), enabled, "{name}");
                }
            }
            for name in ["ask_user", "set_session_title", "compact_context"] {
                assert!(tools.contains(name), "core tool {name}");
            }
        }
        node.shutdown().await.unwrap();
        other.shutdown().await.unwrap();
    }
}
