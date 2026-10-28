use super::*;
use serde_json::json;
use std::collections::BTreeSet;
#[path = "plugins/arguments.rs"]
mod arguments;
#[path = "plugins/assistants.rs"]
mod assistants;
#[path = "plugins/cards.rs"]
mod cards;
#[path = "plugins/code_index.rs"]
mod code_index;
#[path = "plugins/commands.rs"]
mod commands;
#[path = "plugins/discovery.rs"]
mod discovery;
#[path = "plugins/errors.rs"]
mod errors;
#[path = "plugins/failures.rs"]
mod failures;
#[path = "plugins/files.rs"]
mod files;
#[path = "plugins/flows.rs"]
mod flows;
#[path = "plugins/git.rs"]
mod git;
#[path = "plugins/goal_history.rs"]
mod goal_history;
#[path = "plugins/http.rs"]
mod http;
#[path = "plugins/ocr.rs"]
mod ocr;
#[path = "plugins/operations.rs"]
mod operations;
#[path = "plugins/reminders.rs"]
mod reminders;
#[path = "plugins/review.rs"]
mod review;
#[path = "plugins/scheduled_tasks.rs"]
mod scheduled_tasks;

#[tokio::test]
async fn scopes_tool_switches_to_the_execution_node() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let other = Node::start_empty(directory.path().join("other"))
            .await
            .unwrap();
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
        approvals::install(
            &client,
            &[
                "git",
                "commands",
                "files",
                "memory",
                "delegation",
                "worktrees",
            ],
        )
        .await;
        approvals::install(
            &origin,
            &[
                "git",
                "commands",
                "files",
                "memory",
                "delegation",
                "worktrees",
            ],
        )
        .await;
        progress::install(&client).await;
        progress::install(&origin).await;
        let server = Server::scripted_tools(vec![]).await;
        let session = approvals::prepare_packages(&client, &server, &root, &[]).await;
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
        let memory = plugin_tool("memory", "search_memory");
        let common = [
            ("git", "git_status"),
            ("commands", "run_command"),
            ("files", "read_file"),
            ("worktrees", "list_worktrees"),
        ];
        for (index, enabled) in [true, false, true].into_iter().enumerate() {
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
            let mut calls = vec![("tool_search".into(), json!({"query":memory,"limit":1}))];
            if enabled {
                calls.push((memory.clone(), json!({"query":""})));
            }
            server.replace_tools(calls);
            let start = server.requests.lock().unwrap().len();
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
            let details = tool_details(&client, &page).await;
            let searched = details
                .iter()
                .filter(|(name, _)| name == "tool_search")
                .last()
                .unwrap()
                .1
                .result
                .as_ref()
                .unwrap();
            let discovered: BTreeSet<_> = searched["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["name"].as_str().unwrap())
                .collect();
            assert_eq!(
                discovered.contains(memory.as_str()),
                enabled,
                "remote={remote}, enabled={enabled}: {searched}"
            );
            let executed: Vec<_> = details.iter().filter(|(name, _)| name == &memory).collect();
            assert_eq!(executed.len(), if index < 2 { 1 } else { 2 });
            if enabled {
                assert_eq!(searched["tools"][0]["name"], memory);
                assert!(executed.last().unwrap().1.result.as_ref().unwrap()["entries"].is_array());
            }
            assert!(page.approvals.is_empty());
            let requests = server.requests.lock().unwrap();
            let names = |request: &serde_json::Value| -> BTreeSet<String> {
                request["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
                    .collect()
            };
            let initial = names(&requests[start]);
            let tools = names(requests.last().unwrap());
            for (package, name) in common {
                let name = plugin_tool(package, name);
                assert_eq!(initial.contains(&name), enabled, "{name}");
                assert_eq!(tools.contains(&name), enabled, "{name}");
            }
            assert_eq!(initial.contains(&memory), enabled && index > 0);
            assert_eq!(tools.contains(&memory), enabled);
            for (package, name) in [
                ("commands", "write_command"),
                ("delegation", "spawn_agent"),
                ("progress", "update_plan"),
                ("worktrees", "create_worktree"),
            ] {
                assert!(
                    !tools.contains(&plugin_tool(package, name)),
                    "{package}/{name}"
                );
            }
            if !enabled {
                for (_, declarations) in &plugins {
                    assert!(declarations.iter().all(|name| {
                        !initial.contains(name)
                            && !tools.contains(name)
                            && !discovered.contains(name.as_str())
                    }));
                }
            }
            let request = requests.last().unwrap();
            let instructions = node_context(request, "Work mode");
            assert!(!instructions.contains("Use update_plan for substantial multi-step tasks"));
            let memory_rule =
                "When an answer or action depends on earlier preferences or decisions";
            assert_eq!(instructions.contains(memory_rule), enabled && index > 0);
            if enabled && index == 0 {
                assert!(
                    searched["sailry_context"]
                        .as_str()
                        .unwrap()
                        .contains(memory_rule)
                );
            }
            assert!(
                request["messages"][0]
                    .to_string()
                    .contains("Finish with an answer to the user's request")
            );
            let environment = node_context(request, "Session environment");
            assert!(environment.contains("Model:"));
            assert!(environment.contains("Git repository: no"));
            for name in [
                "ask_user",
                "set_session_title",
                "compact_context",
                "list_sessions",
                "read_session",
                "search_session",
            ] {
                assert!(tools.contains(name), "core tool {name}");
            }
        }
        node.shutdown().await.unwrap();
        other.shutdown().await.unwrap();
    }
}
