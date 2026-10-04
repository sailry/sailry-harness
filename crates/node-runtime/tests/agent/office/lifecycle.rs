use super::*;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[tokio::test]
async fn installs_for_new_turns_and_retains_admitted_resources() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
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
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        execute(
            &client,
            Command::RemovePlugin {
                name: "office".into(),
                expected_revision: 1,
            },
        )
        .await;
        let server = Server::turn_tools(vec![
            ("load_skill".into(), json!({"skill":"office:word"})),
            (
                "read_skill_resource".into(),
                json!({"skill":"office:word", "path":"templates/create.py"}),
            ),
        ])
        .await;
        // The session predates installation; each admission must read the current inventory.
        let session = approvals::prepare(&client, &server, &root).await;
        // This fixture exercises core skill loading, independent of tool providers.
        let Output::Plugins(providers) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        for provider in providers.into_iter().filter(|provider| provider.enabled) {
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: provider.name,
                    expected_revision: provider.revision,
                    enabled: false,
                },
            )
            .await;
        }
        let queue = || Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Use the Word skill".into(),
        };
        let Output::QueuedTurn(before) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert!(before.plugins.is_empty());
        let Output::Plugin(installed) = execute(
            &client,
            Command::InstallBundledPlugin {
                name: "office".into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert_eq!(installed.skills.len(), 4);
        assert!(installed.issues.is_empty());
        let Output::QueuedTurn(admitted) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert_eq!(admitted.plugins, vec![installed.summary.reference()]);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "office".into(),
                expected_revision: installed.summary.revision,
                enabled: false,
            },
        )
        .await;
        let Output::QueuedTurn(disabled) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert!(disabled.plugins.is_empty());
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "office".into(),
                expected_revision: installed.summary.revision + 1,
                enabled: true,
            },
        )
        .await;
        let Output::QueuedTurn(enabled) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert_eq!(enabled.plugins, vec![installed.summary.reference()]);
        execute(
            &client,
            Command::RemovePlugin {
                name: "office".into(),
                expected_revision: installed.summary.revision + 2,
            },
        )
        .await;
        let Output::QueuedTurn(removed) = execute(&client, queue()).await else {
            panic!("turn expected")
        };
        assert!(removed.plugins.is_empty());
        for turn in [&before, &admitted, &disabled, &enabled, &removed] {
            assert_eq!(turn.revision, session.revision);
        }
        // Uninstall stops new use without breaking an immutable, already admitted turn.
        execute(&client, Command::StartQueuedTurn { turn: admitted.id }).await;
        let page = finished(&client, session.id, admitted.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == admitted.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2);
        assert!(results[0]["content"].as_str().unwrap().contains("Document"));
        assert!(
            results[1]["content"]
                .as_str()
                .unwrap()
                .contains("document.save")
        );
        assert!(std::path::Path::new(results[1]["directory"].as_str().unwrap()).is_dir());
        let Output::Plugins(inventory) = execute(&client, Command::ListPlugins).await else {
            panic!("inventory expected")
        };
        assert!(!inventory.iter().any(|entry| entry.name == "office"));
        let Output::Plugin(reinstalled) = execute(
            &client,
            Command::InstallBundledPlugin {
                name: "office".into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert!(reinstalled.summary.enabled);
        assert_eq!(reinstalled.summary.digest, installed.summary.digest);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
