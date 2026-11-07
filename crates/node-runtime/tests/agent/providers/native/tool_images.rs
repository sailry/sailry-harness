use super::*;
use serde_json::json;

#[tokio::test]
async fn inspects_without_model_metadata() {
    use sailry_protocol::media::{Action, Source};
    let image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";
    let closed = Arc::new(tokio::sync::Notify::new());
    let event = json!({"id":"vision","object":"chat.completion.chunk","created":1,"model":"fixture-a",
        "choices":[{"index":0,"delta":{"content":"One bright pixel"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":5,"completion_tokens":4,"total_tokens":9}});
    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
    let server = Server::start(
        ModelApi::ChatCompletions,
        Reply::Scripted(Arc::new(vec![
            server::Script {
                bytes: headers.as_bytes().to_vec(),
                ending: server::Ending::Closed(closed),
            },
            server::Script {
                bytes: format!("{headers}data: {event}\n\ndata: [DONE]\n\n").into_bytes(),
                ending: server::Ending::Close,
            },
        ])),
    )
    .await;
    let mut fixture =
        Fixture::packages(ModelApi::ChatCompletions, &server.endpoint, &["media"]).await;
    let mut provider = fixture.provider.clone();
    provider.models.clear();
    let Output::Provider(saved) = execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    assert!(saved.models.is_empty());
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config: SessionConfig {
                effort: Effort::Default,
                ..fixture.session.config.clone()
            },
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
    let Output::Plugin(package) = execute(
        &fixture.client,
        Command::ReadPlugin {
            name: "media".into(),
        },
    )
    .await
    else {
        panic!("media package expected")
    };
    use base64::Engine as _;
    std::fs::write(
        fixture.root.join("input.png"),
        base64::engine::general_purpose::STANDARD
            .decode(image)
            .unwrap(),
    )
    .unwrap();
    let turn = fixture
        .submit("Keep the turn active for image inspection")
        .await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let output = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::UseMedia {
                    turn,
                    session: fixture.session.id,
                    worktree: fixture.session.worktree,
                    action: Action::Inspect {
                        prompt: "Describe this image".into(),
                        source: Source::Path("input.png".into()),
                    },
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: Some(turn),
                    surface: Default::default(),
                    package: package.summary.reference(),
                    worktree: Some(fixture.session.worktree),
                    session: Some(fixture.session.id),
                }),
        )
        .await
        .unwrap();
    assert!(matches!(output, Output::Media(value) if value["text"] == "One bright pixel"));
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let request = &requests[1];
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.body["model"], "fixture-a");
        let users: Vec<_> = request.body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .collect();
        assert_eq!(users.len(), 1);
        let content = users[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert!(
            content
                .iter()
                .any(|part| part["type"] == "text" && part["text"] == "Describe this image")
        );
        let images: Vec<_> = content
            .iter()
            .filter(|part| part["type"] == "image_url")
            .collect();
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0]["image_url"]["url"],
            format!("data:image/png;base64,{image}")
        );
        let serialized = request.body.to_string();
        assert!(!serialized.contains("image omitted"));
        assert!(!serialized.contains("model does not support vision"));
    }
    execute(&fixture.client, Command::StopTurn { turn }).await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
