use super::*;
use goose_provider_types::{
    base::{Provider, collect_stream},
    conversation::message::Message,
    model::ModelConfig,
};
use goose_providers::{
    api_client::{ApiClient, AuthMethod},
    openai::OpenAiProviderBuilder,
    openai_compatible::OpenAiCompatibleProvider,
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use serde_json::json;

#[tokio::test]
async fn inspects_without_model_metadata() {
    use sailry_protocol::media::{Action, Source};
    let image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";
    for remote in [false, true] {
        let closed = Arc::new(tokio::sync::Notify::new());
        let event = json!({"id":"vision","object":"chat.completion.chunk","created":1,"model":"fixture-a",
            "choices":[{"index":0,"delta":{"content":"One bright pixel"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":5,"completion_tokens":4,"total_tokens":9}});
        let headers =
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
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
        let mut fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["media"],
        )
        .await;
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
}

#[tokio::test]
async fn preserves_parallel_image_results() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = Server::start(api, Reply::Text).await;
        let client = ApiClient::new_with_tls(
            server.endpoint.trim_end_matches("/v1").to_owned(),
            AuthMethod::BearerToken(server::KEY.into()),
            None,
        )
        .unwrap();
        let provider: Arc<dyn Provider> = if api == ModelApi::ChatCompletions {
            Arc::new(OpenAiCompatibleProvider::new(
                "deepseek".into(),
                client,
                "v1/".into(),
            ))
        } else {
            Arc::new(
                OpenAiProviderBuilder::new(client)
                    .base_path("v1/responses")
                    .build(),
            )
        };
        let calls = Message::assistant()
            .with_tool_request("capture", Ok(CallToolRequestParams::new("capture")))
            .with_tool_request("read", Ok(CallToolRequestParams::new("read")));
        let response = |id: &str, image: bool| {
            let mut result = CallToolResult::structured(json!({"ok":true}));
            if image {
                result
                    .content
                    .push(ContentBlock::image("AQID", "image/png"));
            }
            Message::user().with_tool_response(id, Ok(result))
        };
        let contents = vec![
            Message::user().with_text("Inspect the tool images"),
            calls,
            response("capture", true),
            response("read", false),
            Message::assistant()
                .with_thinking("Both tools completed", "")
                .with_text("Observed"),
            Message::user().with_text("Continue"),
        ];
        let canonical = serde_json::to_value(&contents).unwrap();
        let config = ModelConfig::new("fixture-a").with_vision_support(true);
        collect_stream(provider.stream(&config, "", &contents, &[]).await.unwrap())
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(&contents).unwrap(), canonical);
        let requests = server.requests.lock().unwrap();
        let body = &requests[0].body;
        if api == ModelApi::ChatCompletions {
            let messages = body["messages"].as_array().unwrap();
            let outputs: Vec<_> = messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["tool_call_id"], "capture");
            assert_eq!(outputs[1]["tool_call_id"], "read");
            assert!(
                outputs
                    .iter()
                    .all(|output| output["content"].as_str().unwrap().contains("true"))
            );
            assert!(messages.iter().any(|message| {
                message["role"] == "user"
                    && message["content"]
                        .to_string()
                        .contains("data:image/png;base64,AQID")
            }));
            let observed = messages
                .iter()
                .find(|message| message["content"] == "Observed")
                .unwrap();
            assert_eq!(observed["reasoning_content"], "Both tools completed");
            assert_eq!(messages.last().unwrap()["content"], "Continue");
        } else {
            let outputs: Vec<_> = body["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "function_call_output")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["call_id"], "capture");
            assert_eq!(outputs[1]["call_id"], "read");
            assert!(
                outputs[0]["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|part| part["type"] == "input_image"
                        && part["image_url"] == "data:image/png;base64,AQID")
            );
            assert!(!outputs[1]["output"].to_string().contains("input_image"));
        }
    }
}
