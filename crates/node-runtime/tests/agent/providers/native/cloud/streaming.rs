use super::*;

#[tokio::test]
async fn without_tools() {
    for remote in [false, true] {
        let (fixture, page) = stream(remote, false).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(text(&page).contains(server::ANSWER));
        let usage = page
            .entries
            .iter()
            .filter_map(|entry| entry.usage.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(usage.len(), 1);
        assert_eq!(
            (usage[0].input, usage[0].output, usage[0].cached_input),
            (16, 4, 6)
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_incomplete_output() {
    for remote in [false, true] {
        let (fixture, page) = stream(remote, true).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Failed,
            "{:?}",
            page.runs
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

async fn stream(remote: bool, incomplete: bool) -> (Fixture, Page) {
    let server = Server::start(ModelApi::Bedrock, events(incomplete)).await;
    let fixture = Fixture::new(remote, ModelApi::Bedrock, &server.endpoint).await;
    let mut provider = fixture.provider.clone();
    provider.models[0].tools = false;
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await;
    let turn = fixture.submit("Stream without tools").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path.ends_with("/converse-stream"));
    drop(requests);
    (fixture, page)
}

fn events(incomplete: bool) -> Reply {
    use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
    use serde_json::json;
    let mut bytes = b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.amazon.eventstream\r\nConnection: close\r\n\r\n".to_vec();
    for (name, body) in [
        ("messageStart", json!({"role":"assistant"})),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"text":server::ANSWER}}),
        ),
        ("contentBlockStop", json!({"contentBlockIndex":0})),
        ("messageStop", json!({"stopReason":"end_turn"})),
        (
            "metadata",
            json!({"usage":{"inputTokens":16,"outputTokens":4,"totalTokens":20,"cacheReadInputTokens":6},"metrics":{"latencyMs":1}}),
        ),
    ] {
        if incomplete && name == "messageStop" {
            break;
        }
        let event = Message::new(body.to_string().into_bytes())
            .add_header(Header::new(
                ":message-type",
                HeaderValue::String("event".into()),
            ))
            .add_header(Header::new(":event-type", HeaderValue::String(name.into())))
            .add_header(Header::new(
                ":content-type",
                HeaderValue::String("application/json".into()),
            ));
        aws_smithy_eventstream::frame::write_message_to(&event, &mut bytes).unwrap();
    }
    Reply::Raw(std::sync::Arc::new(bytes))
}
