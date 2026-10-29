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
                json!({"query": ".", "mode":"content", "regex": true, "globs": ["**/*.rs", "**/*.py", "**/*.js", "**/*.jsx", "**/*.ts", "**/*.tsx", "**/*.go", "**/*.java", "**/*.c", "**/*.h", "**/*.cpp", "**/*.hpp", "**/*.rb", "**/*.php", "**/*.swift", "**/*.kt", "**/*.txt"]}),
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
        let deferred: Vec<_> = [
            ("worktrees", "create_worktree"),
            ("worktrees", "register_worktree"),
            ("worktrees", "remove_worktree"),
            ("worktrees", "list_worktrees"),
            ("git", "git_log"),
            ("git", "list_git_changes"),
        ]
        .into_iter()
        .map(|(package, name)| plugin_tool(package, name))
        .collect();
        let mut script = vec![(
            "tool_search".into(),
            json!({"query":deferred.join(" "),"limit":6}),
        )];
        script.extend(calls.clone());
        let server = Server::tools(script).await;
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
            display_name: None,
            context: 16000,
            output: 2048,
            vision: false,
            tools: true,
            reasoning: false,
            generates: vec![],
            efforts: Vec::new(),
            capabilities: None,
            custom_efforts: false,
            compaction_tokens: None,
            default_effort: sailry_protocol::Effort::Default,
        }];
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let admissions: i64 = node
            .with_storage(|database| {
                Box::pin(async move {
                    database
                        .prepare("SELECT COUNT(*) FROM requests")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(())
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get(0)
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "Inspect the registered worktree".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        provider.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .tools = Some(false);
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
        let details = tool_details(&client, &page).await;
        let results: Vec<_> = details
            .iter()
            .filter(|(name, _)| name != "tool_search")
            .map(|(_, detail)| detail.result.as_ref().unwrap())
            .collect();
        assert_eq!(results.len(), calls.len());
        let discovery: Vec<_> = details
            .iter()
            .filter(|(name, _)| name == "tool_search")
            .collect();
        assert_eq!(discovery.len(), 1);
        let mut loaded: Vec<_> = discovery[0].1.result.as_ref().unwrap()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        let mut expected = deferred.clone();
        loaded.sort();
        expected.sort();
        assert_eq!(loaded, expected);
        assert_eq!(results[0]["data"]["entries"][0]["name"], "资料.txt");
        assert_eq!(
            results[1]["data"]["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line["text"].as_str().unwrap())
                .collect::<String>(),
            "hello 中文 🙂\nneedle\n"
        );
        assert_eq!(results[2]["data"]["matches"][0]["line_number"], 1);
        assert_eq!(results[3]["data"]["kind"], "directory");
        assert_eq!(results[4]["data"]["kind"], "directory");
        assert!(results[5].get("error").is_some());
        assert!(results[6].get("error").is_some());
        assert!(results[7].get("error").is_some());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), calls.len() + 2);
        let catalog = requests[0]["tools"].as_array().unwrap();
        let discovered = requests[1]["tools"].as_array().unwrap();
        let history_tool = discovered
            .iter()
            .find(|tool| tool["function"]["name"] == "git_log")
            .unwrap();
        assert_eq!(
            history_tool["function"]["parameters"]["properties"]["cursor"]["type"],
            "object"
        );
        for name in [
            "tool_search",
            "load_skill",
            "read_skill_resource",
            "list_sessions",
            "read_session",
            "search_session",
            "read_tool_output",
            "set_session_title",
            "ask_user",
        ] {
            assert!(catalog.iter().any(|tool| tool["function"]["name"] == name));
        }
        for name in [
            "manage_skills",
            "manage_mcp",
            "manage_plugins",
            "compact_context",
        ] {
            assert!(!catalog.iter().any(|tool| tool["function"]["name"] == name));
        }
        for name in ["get_goal", "create_goal", "update_goal"] {
            assert!(!catalog.iter().any(|tool| tool["function"]["name"] == name));
        }
        for name in &deferred {
            assert!(
                !catalog
                    .iter()
                    .any(|tool| tool["function"]["name"] == exposed(name))
            );
        }
        let mut expected: Vec<_> = expected
            .iter()
            .map(|name| exposed(name).to_owned())
            .collect();
        expected.sort();
        let mut added: Vec<_> = discovered
            .iter()
            .filter(|tool| !catalog.contains(tool))
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_owned())
            .collect();
        added.sort();
        assert_eq!(
            added, expected,
            "discovery extends the immediately following request"
        );
        assert!(catalog.iter().all(|tool| discovered.contains(tool)));
        for name in ["run_command", "read_command", "stop_command"] {
            let tool = catalog
                .iter()
                .find(|tool| tool["function"]["name"] == name)
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
            assert!(
                !catalog
                    .iter()
                    .any(|tool| tool["function"]["name"] == plugin_tool("commands", name))
            );
        }
        let directory_tool = catalog
            .iter()
            .find(|tool| tool["function"]["name"] == "list_directory")
            .unwrap();
        assert_eq!(
            directory_tool["function"]["parameters"]["properties"]["cursor"]["type"],
            "object"
        );
        let search = catalog
            .iter()
            .find(|tool| tool["function"]["name"] == "search_files")
            .unwrap();
        assert_eq!(
            search["function"]["parameters"]["properties"]["query"]["minLength"],
            1
        );
        for name in [
            "list_git_changes",
            "load_skill",
            "read_skill_resource",
            "set_session_title",
            "list_worktrees",
            "create_worktree",
            "register_worktree",
            "remove_worktree",
        ] {
            assert!(
                discovered
                    .iter()
                    .any(|tool| tool["function"]["name"] == name)
            );
        }
        assert!(
            discovered
                .iter()
                .all(|tool| tool["function"]["name"] != "read_office")
        );
        assert!(
            catalog
                .iter()
                .any(|tool| tool["function"]["name"] == "ask_user")
        );
        assert!(
            catalog
                .iter()
                .filter(|tool| [
                    "list_directory",
                    "read_file",
                    "write_file",
                    "edit_file",
                    "find_files",
                    "search_files"
                ]
                .iter()
                .any(|name| tool["function"]["name"] == *name))
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
        assert_eq!(tool_messages.len(), calls.len() + 1);
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
                let rows: Vec<(String, String, Vec<u8>)> = node
                    .with_storage(move |database| {
                        Box::pin(async move {
                            let mut query = database
                                .prepare("SELECT id,status,body FROM requests ORDER BY rowid LIMIT -1 OFFSET ?1")
                                .await
                                .map_err(crate::database_fixture::fault)?;
                            let mut rows = query
                                .query([admissions])
                                .await
                                .map_err(crate::database_fixture::fault)?;
                            let mut values = Vec::new();
                            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                                values.push((
                                    row.get(0).map_err(crate::database_fixture::fault)?,
                                    row.get(1).map_err(crate::database_fixture::fault)?,
                                    row.get(2).map_err(crate::database_fixture::fault)?,
                                ));
                            }
                            Ok(values)
                        })
                    })
                    .await
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
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 2);
        assert_eq!(
            node.with_storage(move |database| {
                Box::pin(async move {
                    let mut query = database
                        .prepare("SELECT id FROM requests ORDER BY rowid LIMIT -1 OFFSET ?1")
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let mut rows = query
                        .query([admissions])
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let mut values = Vec::new();
                    while let Some(row) =
                        rows.next().await.map_err(crate::database_fixture::fault)?
                    {
                        values.push(
                            row.get::<String>(0)
                                .map_err(crate::database_fixture::fault)?,
                        );
                    }
                    Ok(values)
                })
            })
            .await
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
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 3);
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
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(server.requests.lock().unwrap().len(), calls.len() + 3);
        node.shutdown().await.unwrap();
    }
}
