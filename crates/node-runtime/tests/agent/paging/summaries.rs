use super::*;
use crate::process::Fixture;

fn assert_order(request: &serde_json::Value) {
    turn_context(request);
    let request = task_request(request);
    let messages = request["messages"].as_array().unwrap();
    let kickoff = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "user")
        .find_map(|(index, message)| {
            message["content"]
                .as_str()?
                .find("History fixture")
                .map(|offset| (index, offset))
        })
        .unwrap();
    let answer = messages
        .iter()
        .position(|message| {
            message["role"] == "assistant" && message["content"] == "answer-fixture-a"
        })
        .unwrap();
    let summaries = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "user")
        .flat_map(|(index, message)| {
            message["content"]
                .as_str()
                .unwrap_or("")
                .match_indices("The tool request and response completed.")
                .map(move |(offset, _)| (index, offset))
        })
        .collect::<Vec<_>>();
    assert_eq!(summaries.len(), 10);
    assert!(
        summaries
            .into_iter()
            .all(|position| kickoff < position && position < (answer, 0))
    );
    assert_eq!(messages.last().unwrap()["role"], "user");
    assert_eq!(messages.last().unwrap()["content"], "History fixture");
    assert!(answer < messages.len() - 1);
}

#[tokio::test]
async fn preserves_outcomes_after_tool_summaries() {
    for remote in [false, true] {
        for failed in [false, true] {
            let server = Server::tools(
                (0..26)
                    .map(|_| {
                        (
                            plugin_tool("files", "read_file"),
                            json!({"path":"source.txt"}),
                        )
                    })
                    .collect(),
            )
            .await;
            let fixture = Fixture::files(remote, &server).await;
            std::fs::write(fixture.root.join("source.txt"), "Retained tool evidence").unwrap();
            let first = submit(&fixture.client, fixture.session.id).await;
            let original = finished(&fixture.client, fixture.session.id, first).await;
            assert_eq!(original.runs[0].status, Status::Completed);
            assert_eq!(server.requests.lock().unwrap().len(), 27);
            let rejected = if failed {
                let rejected = Server::http(usize::MAX, 401).await;
                let Output::Providers(providers) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListProviders))
                    .await
                    .unwrap()
                else {
                    panic!("providers expected")
                };
                let mut provider = providers
                    .into_iter()
                    .find(|provider| provider.id == fixture.session.config.provider)
                    .unwrap();
                provider.endpoint = rejected.endpoint.clone();
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutProvider {
                        expected_revision: provider.revision,
                        provider,
                    }))
                    .await
                    .unwrap();
                Some(rejected)
            } else {
                None
            };
            // Current-kickoff tool pairs stay protected. The following turn
            // summarizes older pairs before making its one ordinary request.
            let next = submit(&fixture.client, fixture.session.id).await;
            let page = finished(&fixture.client, fixture.session.id, next).await;
            let run = page.runs.iter().find(|run| run.turn == next).unwrap();
            assert_eq!(
                run.status,
                if failed {
                    Status::Failed
                } else {
                    Status::Completed
                },
                "remote={remote}, failed={failed}: {run:?}"
            );
            if failed {
                assert_eq!(
                    run.error.as_ref().unwrap().details,
                    Some(sailry_protocol::FaultDetails::ModelRequest {
                        reason: sailry_protocol::ModelFailure::Unauthorized,
                    })
                );
            } else {
                assert!(run.error.is_none());
            }
            assert_eq!(&page.entries[..original.entries.len()], original.entries);
            let requests = rejected
                .as_ref()
                .unwrap_or(&server)
                .requests
                .lock()
                .unwrap()
                .clone();
            assert_eq!(requests.len(), if failed { 11 } else { 38 });
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| agent_support::compaction::is_tool_pair(request))
                    .count(),
                10
            );
            let requests = requests
                .iter()
                .filter(|request| !agent_support::compaction::is_tool_pair(request))
                .collect::<Vec<_>>();
            assert_eq!(requests.len(), if failed { 1 } else { 28 });
            assert_order(requests.last().unwrap());
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == next)
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| matches!(part, Part::Text(text) if text == "answer-fixture-a"))
                    .count(),
                usize::from(!failed)
            );
            if failed {
                assert_eq!(server.requests.lock().unwrap().len(), 27);
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
