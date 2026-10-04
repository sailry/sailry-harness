use super::*;
use sailry_protocol::conversation::reference::{Reference, Target};
use serde_json::json;

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
async fn reads_selected_history_through_tools() {
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
        let original = Server::start(false).await;
        let source = approvals::prepare(&client, &original, &root).await;
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
            ("read_session".into(), json!({"session":source.id,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"before":second,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"turn":first,"revision":page.revision,"limit":1})),
            ("read_session".into(), json!({"session":source.id,"turn":first,"revision":page.revision,"before_entry":cursor,"limit":1})),
            ("read_session".into(), json!({"session":forbidden})),
        ]).await;
        let Output::Providers(providers) = client
            .execute(client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == source.config.provider)
            .unwrap();
        provider.endpoint = reader.endpoint.clone();
        provider.models[0].context = 128_000;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let Output::Session(target) = client
            .execute(client.prepare(Command::CreateSession {
                project: source.project,
                worktree: Some(source.worktree),
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
        let results = history(&client, target.id)
            .await
            .entries
            .into_iter()
            .filter(|entry| entry.turn == turn)
            .flat_map(|entry| entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { name, result, .. } if name == "read_session" => Some(result),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(results.len(), 5, "{results:?}");
        assert!(results[0].to_string().contains("Second decision"));
        assert_eq!(results[0]["page"]["next_before"], second.to_string());
        assert!(results[1].to_string().contains("First decision"));
        assert!(results[1]["page"]["next_before"].is_null());
        assert_eq!(results[2]["next_before"], cursor);
        assert!(results[3].to_string().contains("First decision"));
        assert!(results[3]["next_before"].is_null());
        assert!(
            results[4]
                .to_string()
                .contains("conversation was not referenced")
        );
        {
            let requests = reader.requests.lock().unwrap();
            assert_eq!(requests.len(), 6);
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
        assert!(
            !original.requests.lock().unwrap()[0]
                .to_string()
                .contains("\"name\":\"read_session\"")
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
