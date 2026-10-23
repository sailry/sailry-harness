use super::*;
use sailry_protocol::conversation::reference::{Reference, Target};
use serde_json::json;

#[path = "references/scopes.rs"]
mod scopes;

async fn select_server(client: &Client, session: &Session, server: &Server) {
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    let mut provider = providers
        .into_iter()
        .find(|provider| provider.id == session.config.provider)
        .unwrap();
    provider.endpoint = server.endpoint.clone();
    provider.models[0].context = 128_000;
    client
        .execute(client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap();
}

async fn submit(client: &Client, session: &Session, message: Input) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message,
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let page = finished(client, session.id, turn.id).await;
    assert_eq!(
        page.runs
            .iter()
            .find(|run| run.turn == turn.id)
            .unwrap()
            .status,
        Status::Completed,
        "{page:?}"
    );
    turn.id
}

#[tokio::test]
async fn reads_history_with_and_without_mentions() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let repository = git2::Repository::init(&root).unwrap();
        let tree = repository
            .find_tree(repository.index().unwrap().write_tree().unwrap())
            .unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let commit = repository
            .commit(Some("HEAD"), &signature, &signature, "Initial", &tree, &[])
            .unwrap();
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
        let original = Server::start(false).await;
        let source = approvals::prepare_packages(&client, &original, &root, &[]).await;
        let first = submit(
            &client,
            &source,
            "First decision: use the shared editor 中文".into(),
        )
        .await;
        let second = submit(
            &client,
            &source,
            "Second decision: verify remote behavior".into(),
        )
        .await;
        let page = history(&client, source.id).await;
        let cursor = page
            .entries
            .iter()
            .rfind(|entry| entry.turn == first)
            .unwrap()
            .sequence;
        let forbidden = SessionId::new();
        let reader = Server::tools(vec![
            ("list_sessions".into(), json!({"text":"First decision"})),
            ("read_session".into(), json!({"session":source.id,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"before":second,"revision":page.revision,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"turn":first,"revision":page.revision,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"turn":first,"revision":page.revision,"before_entry":cursor,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"limit":100_000})),
            ("search_session".into(), json!({"session":source.id,"text":"shared editor"})),
            ("read_session".into(), json!({"session":forbidden})),
        ]).await;
        select_server(&client, &source, &reader).await;
        let Output::Worktree(worktree) = client
            .execute(
                client.prepare(Command::CreateWorktree {
                    project: source.project.unwrap(),
                    path: directory
                        .path()
                        .join("history-reader")
                        .to_str()
                        .unwrap()
                        .into(),
                    branch: "history-reader".into(),
                    commit: commit.to_string(),
                }),
            )
            .await
            .unwrap()
        else {
            panic!("worktree expected")
        };
        let Output::Session(target) = client
            .execute(client.prepare(Command::CreateSession {
                project: source.project,
                worktree: Some(worktree.id),
                config: Some(source.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let turn = submit(
            &client,
            &target,
            format!("Summarize conversation {} and continue its task", source.id).into(),
        )
        .await;
        let results = history(&client, target.id)
            .await
            .entries
            .into_iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { name, result, .. }
                    if ["list_sessions", "read_session", "search_session"]
                        .contains(&name.as_str()) =>
                {
                    Some(result)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 8, "{results:?}");
        assert_eq!(results[0]["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(results[0]["sessions"][0]["session"], source.id.to_string());
        assert!(results[0]["sessions"][0].get("config").is_none());
        assert!(results[1].to_string().contains("Second decision"));
        assert_eq!(results[1]["page"]["next_before"], second.to_string());
        assert!(results[2].to_string().contains("First decision"));
        assert!(results[2]["page"]["next_before"].is_null());
        assert_eq!(results[3]["next_before"], cursor);
        assert!(results[4].to_string().contains("First decision"));
        assert!(results[4]["next_before"].is_null());
        assert!(results[5].to_string().contains("First decision"));
        assert!(results[5].to_string().contains("Second decision"));
        assert!(results[5]["page"]["next_before"].is_null());
        assert_eq!(results[6]["matches"].as_array().unwrap().len(), 1);
        assert_eq!(results[6]["matches"][0]["turn"], first.to_string());
        assert_eq!(results[7]["error"]["code"], "not_found");
        {
            let requests = reader.requests.lock().unwrap();
            assert_eq!(requests.len(), 9);
            assert!(
                requests
                    .last()
                    .unwrap()
                    .to_string()
                    .contains("First decision")
            );
            assert!(
                requests
                    .last()
                    .unwrap()
                    .to_string()
                    .contains("Second decision")
            );
        }
        for name in ["list_sessions", "read_session", "search_session"] {
            assert!(
                original.requests.lock().unwrap()[0]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["function"]["name"] == name)
            );
        }
        let Output::Session(selected) = client
            .execute(client.prepare(Command::CreateSession {
                project: source.project,
                worktree: Some(worktree.id),
                config: Some(source.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let selected_turn = submit(
            &client,
            &selected,
            Input {
                text: "Summarize @Reference history".into(),
                references: vec![Reference {
                    target: Target::Session(source.id),
                    label: "Reference history".into(),
                }],
                attachments: vec![],
            },
        )
        .await;
        assert!(history(&client, selected.id).await.entries.iter().any(|entry| {
            entry.turn == selected_turn && entry.parts.iter().any(|part| {
                matches!(part, Part::ToolResult {name,result,..} if name == "read_session" && result.to_string().contains("First decision"))
            })
        }));
        let Output::Session(current) = client
            .execute(client.prepare(Command::ReadSession { session: target.id }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(current.worktree, worktree.id);
        assert_eq!(current.config, target.config);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
