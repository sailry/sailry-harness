use super::*;
use serde_json::{Value, json};

#[tokio::test]
async fn reads_bound_worktree() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("资料.txt"), "hello 中文 🙂\nneedle\n").unwrap();
        std::fs::write(directory.path().join("outside.txt"), "outside scope").unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport);
        let calls = vec![
            (
                plugin_tool("files", "list_directory"),
                json!({"path": ".", "cursor": null}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "资料.txt"}),
            ),
            (
                plugin_tool("files", "search_files"),
                json!({"query": ".", "regex": true, "globs": ["**/*.rs", "**/*.py", "**/*.js", "**/*.jsx", "**/*.ts", "**/*.tsx", "**/*.go", "**/*.java", "**/*.c", "**/*.h", "**/*.cpp", "**/*.hpp", "**/*.rb", "**/*.php", "**/*.swift", "**/*.kt", "**/*.txt"]}),
            ),
            (plugin_tool("git", "git_status"), json!({})),
            (
                plugin_tool("git", "git_log"),
                json!({"limit": 3, "cursor": null}),
            ),
            (plugin_tool("git", "git_diff"), json!({"path": "资料.txt"})),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "../outside.txt"}),
            ),
            (
                plugin_tool("files", "read_file"),
                json!({"path": "资料.txt", "worktree": WorktreeId::new()}),
            ),
        ];
        let server = Server::tools(calls.clone()).await;
        let (session, mut provider) = configured(&client, &server.endpoint, &root).await;
        client
            .execute(client.prepare(Command::ReadPlugin {
                name: "office".into(),
            }))
            .await
            .unwrap();
        provider.default_model = "fixture-a".into();
        provider.models = vec![Model {
            id: "fixture-a".into(),
            context: 16000,
            output: 2048,
            vision: false,
            tools: true,
            reasoning: false,
            web_search: false,
            generates: vec![],
            efforts: Vec::new(),
            custom_efforts: false,
            default_effort: sailry_protocol::Effort::Default,
        }];
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let database =
            rusqlite::Connection::open(directory.path().join("node/storage/node.sqlite3")).unwrap();
        let admissions: i64 = database
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
            .unwrap();
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "Inspect the registered worktree".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        provider.models[0].tools = false;
        let changed = client.prepare(Command::PutProvider {
            provider,
            expected_revision: 2,
        });
        client.execute(changed.clone()).await.unwrap();
        let start = client.prepare(Command::StartQueuedTurn { turn: turn.id });
        client.execute(start.clone()).await.unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), calls.len());
        assert_eq!(results[0]["data"]["entries"][0]["name"], "资料.txt");
        assert_eq!(results[1]["data"]["text"], "hello 中文 🙂\nneedle\n");
        assert_eq!(results[2]["data"]["matches"][0]["line_number"], 1);
        assert_eq!(results[3]["data"]["kind"], "directory");
        assert_eq!(results[4]["data"]["kind"], "directory");
        assert!(results[5].get("error").is_some());
        assert!(results[6].get("error").is_some());
        assert!(results[7].get("error").is_some());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), calls.len() + 1);
        let catalog = requests[0]["tools"].as_array().unwrap();
        let history_tool = catalog
            .iter()
            .find(|tool| tool["function"]["name"] == plugin_tool("git", "git_log"))
            .unwrap();
        assert_eq!(
            history_tool["function"]["parameters"]["properties"]["cursor"]["type"],
            json!(["object", "null"])
        );
        let mut expected: Vec<String> = [
            "list_skills",
            "discover_skills",
            "install_skill",
            "update_skill",
            "uninstall_skill",
            "load_skill",
            "read_skill_resource",
            "compact_context",
            "set_session_title",
            "ask_user",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let packages: &[(&str, &[&str])] = &[
            ("reminders", &["reminders"]),
            ("scheduled-tasks", &["scheduled_tasks"]),
            ("goals", &["create_goal", "get_goal", "update_goal"]),
            ("commands", &["run_command", "read_command", "stop_command"]),
            (
                "memory",
                &[
                    "search_memory",
                    "review_memories",
                    "save_memory",
                    "forget_memory",
                    "consolidate_memories",
                ],
            ),
            (
                "files",
                &[
                    "list_directory",
                    "read_file",
                    "write_file",
                    "search_files",
                    "get_office_runtime",
                    "read_office",
                    "export_pdf",
                ],
            ),
            ("progress", &["update_plan"]),
            ("delegation", &["spawn_agent"]),
            (
                "worktrees",
                &[
                    "list_worktrees",
                    "create_worktree",
                    "register_worktree",
                    "remove_worktree",
                ],
            ),
            ("git", &["git_status", "git_diff", "git_log"]),
        ];
        for (package, tools) in packages {
            expected.extend(tools.iter().map(|tool| plugin_tool(package, tool)));
        }
        let mut names: Vec<String> = catalog
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
            .collect();
        expected.sort();
        names.sort();
        assert_eq!(names, expected);
        for name in ["get_goal", "create_goal", "update_goal"] {
            assert!(
                catalog
                    .iter()
                    .any(|tool| tool["function"]["name"] == plugin_tool("goals", name))
            );
        }
        for name in ["run_command", "read_command", "stop_command"] {
            let tool = catalog
                .iter()
                .find(|tool| tool["function"]["name"] == plugin_tool("commands", name))
                .unwrap();
            assert!(
                tool["function"]["parameters"]["properties"]
                    .get("worktree")
                    .is_none()
            );
            assert!(
                tool["function"]["parameters"]["properties"]
                    .get("session")
                    .is_none()
            );
            assert!(!catalog.iter().any(|tool| tool["function"]["name"] == name));
        }
        let directory_tool = catalog
            .iter()
            .find(|tool| tool["function"]["name"] == plugin_tool("files", "list_directory"))
            .unwrap();
        assert_eq!(
            directory_tool["function"]["parameters"]["properties"]["cursor"]["type"],
            json!(["object", "null"])
        );
        let search = catalog
            .iter()
            .find(|tool| tool["function"]["name"] == plugin_tool("files", "search_files"))
            .unwrap();
        assert_eq!(
            search["function"]["parameters"]["properties"]["query"]["minLength"],
            1
        );
        for name in [
            plugin_tool("files", "read_office").as_str(),
            plugin_tool("files", "get_office_runtime").as_str(),
            plugin_tool("files", "export_pdf").as_str(),
            "load_skill",
            "read_skill_resource",
            "compact_context",
            "set_session_title",
            plugin_tool("worktrees", "list_worktrees").as_str(),
            plugin_tool("worktrees", "create_worktree").as_str(),
            plugin_tool("worktrees", "register_worktree").as_str(),
            plugin_tool("worktrees", "remove_worktree").as_str(),
        ] {
            assert!(catalog.iter().any(|tool| tool["function"]["name"] == name));
        }
        assert!(
            catalog
                .iter()
                .any(|tool| tool["function"]["name"] == "ask_user")
        );
        assert!(
            catalog
                .iter()
                .filter(
                    |tool| ["list_directory", "read_file", "write_file", "search_files"]
                        .iter()
                        .any(|name| tool["function"]["name"] == plugin_tool("files", name))
                )
                .all(|tool| {
                    tool["function"]["parameters"]["properties"]
                        .get("worktree")
                        .is_none()
                })
        );
        let tool_messages: Vec<Value> = requests.last().unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .cloned()
            .collect();
        assert_eq!(tool_messages.len(), calls.len());
        assert!(
            tool_messages
                .iter()
                .any(|message| message["content"].as_str().unwrap().contains("hello 中文"))
        );
        assert_eq!(
            std::fs::read_to_string(root.join("资料.txt")).unwrap(),
            "hello 中文 🙂\nneedle\n"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("outside.txt")).unwrap(),
            "outside scope"
        );
        // Terminal status is published before the goals package's after-turn callback.
        let settled = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let rows = database
                    .prepare(
                        "SELECT id,status,body FROM requests ORDER BY rowid LIMIT -1 OFFSET ?1",
                    )
                    .unwrap()
                    .query_map([admissions], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                assert!(rows.len() <= 4, "unexpected durable admissions");
                if rows.len() == 4 && rows.iter().all(|(_, status, _)| status == "completed") {
                    break rows;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("turn callback did not settle");
        let admitted: Vec<Request> = settled
            .iter()
            .map(|(_, _, body)| serde_json::from_slice(body).unwrap())
            .collect();
        for request in [&request, &changed, &start] {
            assert!(admitted.contains(request));
        }
        let callback = admitted
            .iter()
            .find(|request| matches!(request.command, Command::CallPlugin { .. }))
            .unwrap();
        let Command::CallPlugin { handler, input } = &callback.command else {
            unreachable!();
        };
        assert_eq!(handler, "completed");
        assert_eq!(input["turn"], json!(turn.id));
        assert_eq!(input["status"], json!(Status::Completed));
        let context = callback.plugin.as_ref().unwrap();
        assert_eq!(context.package.name, "goals");
        assert_eq!(context.session, Some(session.id));
        assert_eq!(context.worktree, Some(session.worktree));
        assert!(context.turn.is_none());
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::QueuedTurn(turn.clone())
        );
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 1);
        assert_eq!(
            database
                .prepare("SELECT id FROM requests ORDER BY rowid LIMIT -1 OFFSET ?1")
                .unwrap()
                .query_map([admissions], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            settled.into_iter().map(|(id, _, _)| id).collect::<Vec<_>>()
        );
        let Output::QueuedTurn(next) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "Continue without tools".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&client, session.id, next.id).await;
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 2);
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .get("tools")
                .is_none()
        );
        drop(database);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 2);
        node.shutdown().await.unwrap();
    }
}
