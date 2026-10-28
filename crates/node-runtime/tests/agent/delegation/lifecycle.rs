use super::*;
use std::collections::BTreeSet;

#[tokio::test]
async fn drains_with_full_root_pool() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
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
        let parent = Server::parallel(vec![
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Wait A"}),
            ),
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role": "review", "task": "Wait B"}),
            ),
        ])
        .await;
        let child = Server::start(true).await;
        let (session, provider, role) = setup(&client, &root, &parent, &child).await;
        capacity(&client, "delegation", 16).await;
        let captured = session.roles.providers.clone();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].id, provider.id);
        let config = session.config.clone();
        let mut parents = Vec::new();
        for _ in 0..8 {
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: session.project,
                    worktree: Some(session.worktree),
                    config: Some(session.config.clone()),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            assert_eq!(session.config, config);
            assert!(session.roles.profiles.contains(&role));
            assert_eq!(session.roles.providers, captured);
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 1,
                    message: "Delegate independent waiting tasks".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            assert_eq!(turn.config, session.config);
            assert_eq!(turn.roles, session.roles);
            parents.push(turn);
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            while parent.requests.lock().unwrap().len() < 8 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("all root workers must reach the model");
        // Child startup has its own deadline after all root workers are occupied.
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            while child.requests.lock().unwrap().len() < 16 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        if ready.is_err() {
            let pages = tokio::time::timeout(Duration::from_secs(2), async {
                let mut pages = Vec::new();
                for turn in &parents {
                    let page = history(&client, turn.session).await;
                    pages.push((turn.id, page.runs, page.entries, page.children));
                }
                pages
            })
            .await;
            panic!(
                "model request deadline: remote={remote}, parent={}, child={}, histories={pages:#?}",
                parent.requests.lock().unwrap().len(),
                child.requests.lock().unwrap().len(),
            );
        }
        let mut child_ids = BTreeSet::new();
        for turn in parents.iter().take(4) {
            client
                .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                .await
                .unwrap();
            let page = finished(&client, turn.session, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Cancelled);
            for child in children(&client, turn.session, 2).await {
                assert!(child_ids.insert(child.id));
                let page = history(&client, child.id).await;
                assert_eq!(page.runs.len(), 1);
                assert_eq!(page.runs[0].status, Status::Cancelled);
                assert!(page.approvals.is_empty());
            }
        }
        for turn in parents.iter().skip(4) {
            assert_eq!(
                history(&client, turn.session).await.runs[0].status,
                Status::Running
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(node.local());
        for turn in parents.iter().skip(4) {
            assert_eq!(
                history(&client, turn.session).await.runs[0].status,
                Status::Interrupted
            );
            for child in children(&client, turn.session, 2).await {
                assert!(child_ids.insert(child.id));
                let page = history(&client, child.id).await;
                assert_eq!(page.runs.len(), 1);
                assert_eq!(page.runs[0].status, Status::Interrupted);
            }
        }
        assert_eq!(child_ids.len(), 16);
        let requests = child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 16);
        for child in child_ids {
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request["prompt_cache_key"] == json!(child))
                    .count(),
                1,
                "each child reaches the model once"
            );
        }
        let requests = parent.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 16);
        let spawn = plugin_tool("delegation", "spawn_agent");
        for turn in &parents {
            let requests: Vec<_> = requests
                .iter()
                .filter(|request| request["prompt_cache_key"] == json!(turn.session))
                .collect();
            assert_eq!(requests.len(), 2);
            assert!(
                requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == "tool_search")
            );
            assert!(
                requests[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|tool| tool["function"]["name"] != spawn)
            );
            assert!(
                requests[1]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == spawn)
            );
            assert!(
                requests[1]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| {
                        message["role"] == "tool" && message["tool_call_id"] == "discovery-fixture"
                    })
            );
        }
        node.shutdown().await.unwrap();
    }
}
