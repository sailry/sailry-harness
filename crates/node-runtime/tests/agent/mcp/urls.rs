use super::*;
use question::{Answer, Response, State};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn restores_consent_history() {
    for remote in [false, true] {
        for sse in [false, true] {
            let peer =
                input::Server::url("https://example.invalid/continue?task=fixture", sse).await;
            let model = Server::tools(vec![(alias("input", "read"), json!({})); 3]).await;
            let fixture = process::Fixture::new(remote, &model).await;
            peer.package(&fixture.root.join("package"));
            install(&fixture, 0).await;

            let (request, turn) = submit(&fixture, false).await;
            let mut accepted = Vec::new();
            for response in [
                Response::Answer(Answer::Opened),
                Response::Decline,
                Response::Cancel,
            ] {
                let (page, pending) = questions::pending(&fixture.client, fixture.session.id).await;
                let part = &page
                    .entries
                    .iter()
                    .find(|entry| entry.id == pending.entry)
                    .unwrap()
                    .parts[pending.index];
                let Part::ToolCall { arguments, .. } = part else {
                    panic!("MCP input expected")
                };
                let spec: question::Spec = serde_json::from_value(arguments.clone()).unwrap();
                assert!(
                    matches!(spec.input, question::Input::Url { ref url, .. } if url == "https://example.invalid/continue?task=fixture")
                );
                fixture
                    .controller
                    .handle()
                    .disconnect(fixture.node.id())
                    .await;
                let request = fixture.client.prepare(Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: pending.id,
                    response,
                });
                let receipt = fixture.client.execute(request.clone()).await.unwrap();
                accepted.push((request, receipt));
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(page.questions[0].state, State::Answered(Answer::Opened));
            assert_eq!(page.questions[1].state, State::Declined);
            assert_eq!(page.questions[2].state, State::Cancelled);
            let output = results(&page);
            let native: Vec<_> = output
                .iter()
                .filter_map(|value| value.get("output"))
                .collect();
            assert_eq!(
                native,
                vec![
                    &json!({"action":"accept"}),
                    &json!({"action":"decline"}),
                    &json!({"action":"cancel"})
                ]
            );
            let requests = serde_json::to_string(&*model.requests.lock().unwrap()).unwrap();
            assert!(!requests.contains("example.invalid/continue"));
            assert!(!requests.contains("mcp_elicitation"));
            let process::Fixture {
                directory,
                node,
                controller,
                client,
                session,
                ..
            } = fixture;
            drop(client);
            node.shutdown().await.unwrap();
            let node = Node::start(directory.path().join("node")).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(history(&client, session.id).await, page);
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn)
            );
            for (request, receipt) in accepted {
                assert_eq!(client.execute(request).await.unwrap(), receipt);
            }
            assert_eq!(peer.calls.load(Ordering::SeqCst), 3);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
