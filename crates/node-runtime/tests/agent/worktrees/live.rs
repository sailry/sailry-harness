//! Explicit, billable acceptance through the normal Node and ADK execution path.
use super::*;

#[tokio::test]
#[ignore = "requires SAILRY_ACCEPTANCE_PROFILE and incurs real model usage"]
async fn delegates_checkout() {
    let model =
        std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("an explicit acceptance model is required");
    let (source, secret) = provider_fixture::load(&model);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        init(&root);
        let expected = uuid::Uuid::new_v4().to_string();
        std::fs::write(root.join("source.txt"), &expected).unwrap();
        let (node, controller, client) = connect(directory.path(), remote).await;
        let (session, mut selected) = configured(&client, &source.endpoint, &root).await;
        selected.api = source.api;
        selected.authentication = source.authentication;
        selected.options = source.options.clone();
        selected.models = source
            .models
            .iter()
            .filter(|item| item.id == model)
            .cloned()
            .collect();
        selected.default_model = model.clone();
        let Output::Provider(selected) = execute(
            &client,
            Command::SaveProvider {
                expected_revision: selected.revision,
                provider: selected,
                secret: Some(secret.clone()),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = session.config;
        config.model = model.clone();
        config.credential = selected.credential;
        config.effort = Effort::Low;
        config.permission = Permission::Full;
        let Output::Session(session) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = execute(&client, Command::SubmitTurn {
            session: session.id, expected_revision: session.revision,
            message: "Run this authorized worktree integration task. Use git_status to obtain the current head and index_revision, then create_worktree with branch luna-task and include_changes=true. Use list_worktrees to verify the returned registered ID. Call spawn_agent with role=null and worktree set to that exact returned ID. Ask the child to read source.txt in its own worktree and use write_file to create child.txt containing exactly those bytes, then report completion. After the child finishes, use read_file and write_file in your original worktree to create parent.txt containing exactly source.txt. Use the dedicated tools, not shell commands. Keep source.txt unchanged. Do not create child.txt in the parent directory, create parent.txt in the child directory, merge, or remove the worktree. This request explicitly authorizes the child delegation and these file writes. Finish with a short completion report.".into(),
        }).await else { panic!("turn expected") };
        let started = std::time::Instant::now();
        let mut seen = std::collections::HashSet::new();
        let page = loop {
            let page = history(&client, session.id).await;
            for entry in &page.entries {
                for (index, part) in entry.parts.iter().enumerate() {
                    if seen.insert((entry.id.clone(), index)) {
                        match part {
                            Part::ToolCall { name, .. } => {
                                println!("{model} remote={remote} call={name}")
                            }
                            Part::ToolResult { name, result, .. } => println!(
                                "{model} remote={remote} result={name} error={}",
                                result.get("error").is_some()
                            ),
                            _ => {}
                        }
                    }
                }
            }
            if page.runs.iter().any(|run| {
                run.turn == turn.id
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                break page;
            }
            if started.elapsed() > Duration::from_secs(360) {
                execute(&client, Command::StopTurn { turn: turn.id }).await;
                break finished(&client, session.id, turn.id).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        let mut child_pages = Vec::new();
        for child in &page.children {
            child_pages.push(history(&client, child.run.session).await);
        }
        if let Some(path) = std::env::var_os("SAILRY_WORKTREE_EVIDENCE") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join(if remote { "remote.json" } else { "local.json" }),
                serde_json::to_vec_pretty(&json!({"model":model,"remote":remote,"elapsed_ms":started.elapsed().as_millis(),"parent":page,"children":child_pages})).unwrap()).unwrap();
        }
        // Stop the real services before checking outcomes, including failure paths.
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "real model task did not complete"
        );
        assert_eq!(page.runs[0].worktree, session.worktree);
        let calls: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolCall { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        for name in [
            plugin_tool("git", "git_status").as_str(),
            plugin_tool("worktrees", "create_worktree").as_str(),
            plugin_tool("worktrees", "list_worktrees").as_str(),
            plugin_tool("delegation", "spawn_agent").as_str(),
            plugin_tool("files", "read_file").as_str(),
            plugin_tool("files", "write_file").as_str(),
        ] {
            assert!(calls.contains(&name), "missing real model call: {name}");
        }
        let tree: Worktree = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolResult { name, result, .. }
                    if name == &plugin_tool("worktrees", "create_worktree") =>
                {
                    serde_json::from_value(result["data"].clone()).ok()
                }
                _ => None,
            })
            .expect("real model must create a registered worktree");
        assert_ne!(tree.id, session.worktree);
        assert_eq!(page.children.len(), 1);
        assert_eq!(page.children[0].run.worktree, tree.id);
        assert_eq!(page.children[0].run.status, Status::Completed);
        assert_eq!(child_pages[0].runs[0].worktree, tree.id);
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::ToolCall { name, arguments, .. }
                if name == &crate::agent_support::plugin_tool("delegation", "spawn_agent") && arguments["worktree"] == json!(tree.id))
                })
        );
        for name in [
            plugin_tool("files", "read_file"),
            plugin_tool("files", "write_file"),
        ] {
            assert!(
                child_pages[0]
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(part, Part::ToolCall { name: called, .. } if called == &name)
                    }),
                "missing child model call: {name}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(root.join("source.txt")).unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read_to_string(root.join("parent.txt")).unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&tree.path).join("child.txt")).unwrap(),
            expected
        );
        assert!(!root.join("child.txt").exists());
        assert!(!Path::new(&tree.path).join("parent.txt").exists());
        println!(
            "{model} remote={remote}: created worktree, delegated child and verified isolated file outcomes in {} ms",
            started.elapsed().as_millis()
        );
    }
}
