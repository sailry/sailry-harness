use super::*;

#[path = "native/anthropic_search.rs"]
mod anthropic_search;
#[path = "native/assistants.rs"]
mod assistants;
#[path = "native/cloud.rs"]
mod cloud;
#[path = "native/function_tools.rs"]
mod function_tools;
#[path = "native/grounding.rs"]
mod grounding;
#[path = "native/phases.rs"]
mod phases;
#[path = "native/reasoning.rs"]
mod reasoning;
#[path = "native/search_packages.rs"]
mod search_packages;
#[path = "native/server.rs"]
mod server;
#[path = "native/tool_images.rs"]
mod tool_images;
#[path = "native/usage.rs"]
mod usage;
#[path = "native/web_search.rs"]
mod web_search;
use server::{Reply, Server};
#[path = "native/opencode.rs"]
mod opencode;

struct Fixture {
    directory: tempfile::TempDir,
    root: std::path::PathBuf,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
    provider: Provider,
}

impl Fixture {
    async fn new(remote: bool, api: ModelApi, endpoint: &str) -> Self {
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
        let (mut session, mut provider) = configured(&client, endpoint, &root).await;
        provider.api = api;
        provider.options = cloud::options(api);
        let effort = if matches!(api, ModelApi::Bedrock | ModelApi::AzureAi) {
            Effort::Default
        } else {
            Effort::High
        };
        session.config.effort = effort;
        provider.models.push(Model {
            id: "fixture-a".into(),
            context: 2048,
            output: 512,
            vision: true,
            tools: true,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![effort],
            custom_efforts: false,
            default_effort: effort,
        });
        provider.default_model = "fixture-a".into();
        let Output::Provider(provider) = execute(
            &client,
            Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(Secret::new(server::KEY.into())),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        session.config.credential = provider.credential.clone();
        let Output::Session(session) = execute(
            &client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config: session.config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        Self {
            directory,
            root,
            node,
            controller,
            client,
            session,
            provider,
        }
    }

    async fn submit(&self, message: &str) -> TurnId {
        let Output::QueuedTurn(turn) = execute(
            &self.client,
            Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message: message.into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        turn.id
    }
}

#[tokio::test]
async fn versioned_anthropic_endpoint() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, Reply::Text).await;
        let endpoint = format!("{}/gateway/v1/", server.endpoint);
        let fixture = Fixture::new(remote, ModelApi::Anthropic, &endpoint).await;
        let turn = fixture.submit("Read the greeting").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(text(&page).contains(server::ANSWER));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/gateway/v1/messages");
        assert_eq!(requests[0].headers["x-api-key"], server::KEY);
    }
}

#[tokio::test]
async fn restores_authenticated_history() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Text).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("第一条 🙂").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            let Output::QueuedTurn(queued) = execute(
                &fixture.client,
                Command::QueueTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "第二条 🙂".into(),
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            let mut changed = fixture.provider.clone();
            changed.endpoint = "http://127.0.0.1:9/changed".into();
            changed.models[0].output = 16;
            changed.models[0].reasoning = false;
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: changed.revision,
                    provider: changed,
                },
            )
            .await;
            fixture.controller.close().await.unwrap();
            let client = Client::new(fixture.node.local());
            execute(&client, Command::StartQueuedTurn { turn: queued.id }).await;
            let page = finished(&client, fixture.session.id, queued.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                for request in requests.iter() {
                    match api {
                        ModelApi::DeepSeek => {
                            assert_eq!(request.path, "/v1/chat/completions");
                            assert_eq!(
                                request.headers["authorization"],
                                format!("Bearer {}", server::KEY)
                            );
                            assert_eq!(request.body["model"], "fixture-a");
                            assert_eq!(request.body["max_tokens"], 512);
                            assert_eq!(request.body["thinking"]["type"], "enabled");
                            assert_eq!(request.body["reasoning_effort"], "high");
                        }
                        ModelApi::Anthropic => {
                            assert_eq!(request.path, "/v1/messages");
                            assert_eq!(request.headers.get("x-api-key").unwrap(), server::KEY);
                            assert_eq!(request.body["model"], "fixture-a");
                            assert_eq!(request.body["max_tokens"], 512);
                            assert_eq!(request.body["thinking"]["type"], "adaptive");
                            assert_eq!(request.body["output_config"]["effort"], "high");
                        }
                        ModelApi::Gemini => {
                            assert_eq!(
                                request.path,
                                "/v1/models/fixture-a:streamGenerateContent?alt=sse"
                            );
                            assert_eq!(request.headers.get("x-goog-api-key").unwrap(), server::KEY);
                            assert_eq!(request.body["generationConfig"]["maxOutputTokens"], 512);
                            assert_eq!(
                                request.body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
                                "high"
                            );
                        }
                        _ => unreachable!(),
                    }
                    assert!(!request.body.to_string().contains(server::KEY));
                }
                let second = requests[1].body.to_string();
                assert!(
                    second.contains("第一条 🙂")
                        && second.contains(server::ANSWER)
                        && second.contains("第二条 🙂")
                );
            }
            let profile = fixture.node.profile().to_owned();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(profile).await.unwrap();
            assert_eq!(
                history(&Client::new(node.local()), fixture.session.id).await,
                page
            );
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            node.shutdown().await.unwrap();
            drop(fixture.directory);
        }
    }
}

#[tokio::test]
async fn executes_tools_with_adk_history() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Tool).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
            let turn = fixture.submit("Read native.txt").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::ToolResult { .. }))
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].body.to_string().contains("read_file"));
            assert!(requests[1].body.to_string().contains("工具内容 🙂"));
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn bounds_transient_failure_retries() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Failure).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("failure fixture").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Failed,
                "{:?}",
                page.runs
            );
            assert_eq!(server.requests.lock().unwrap().len(), 6);
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[tokio::test]
async fn cancels_active_requests() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let closed = Arc::new(tokio::sync::Notify::new());
            let server = Server::start(api, Reply::Wait(closed.clone())).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            let turn = fixture.submit("cancellation fixture").await;
            tokio::time::timeout(Duration::from_secs(10), async {
                while server.requests.lock().unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            execute(&fixture.client, Command::StopTurn { turn }).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
            tokio::time::timeout(Duration::from_secs(10), closed.notified())
                .await
                .unwrap();
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_unsupported_effort() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Gemini, Reply::Text).await;
        let mut fixture = Fixture::new(remote, ModelApi::Gemini, &server.endpoint).await;
        let mut provider = fixture.provider.clone();
        provider.models[0].efforts.push(Effort::Max);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        fixture.session.config.effort = Effort::Max;
        let request = fixture.client.prepare(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config: fixture.session.config.clone(),
        });
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn confines_credentials() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let target = Server::start(api, Reply::Text).await;
            let origin = Server::start(api, Reply::Redirect(target.endpoint.clone())).await;
            let fixture = Fixture::new(remote, api, &origin.endpoint).await;
            let turn = fixture.submit("redirect fixture").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert!(
                target.requests.lock().unwrap().is_empty(),
                "{api:?} forwarded a credential to another origin"
            );
            assert_eq!(page.runs.last().unwrap().status, Status::Failed);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn streams_fragmented_unicode() {
    use adk_core::{Content, Llm, LlmRequest};
    use adk_model::{
        anthropic::{AnthropicClient, AnthropicConfig},
        deepseek::{DeepSeekClient, DeepSeekConfig},
        gemini::GeminiModel,
    };
    use futures::StreamExt;
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        let server = Server::start(api, Reply::Text).await;
        let model: Arc<dyn Llm> = match api {
            ModelApi::Anthropic => Arc::new(
                AnthropicClient::new(
                    AnthropicConfig::new(server::KEY, "fixture-a").with_base_url(&server.endpoint),
                )
                .unwrap(),
            ),
            ModelApi::Gemini => Arc::new(
                GeminiModel::new_with_base_url(
                    server::KEY,
                    "fixture-a",
                    format!("{}/", server.endpoint),
                )
                .unwrap(),
            ),
            ModelApi::DeepSeek => Arc::new(
                DeepSeekClient::new(
                    DeepSeekConfig::new(server::KEY, "fixture-a").with_base_url(&server.endpoint),
                )
                .unwrap(),
            ),
            _ => unreachable!(),
        };
        let mut stream = model
            .generate_content(
                LlmRequest::new("fixture-a", vec![Content::new("user").with_text("hello")]),
                true,
            )
            .await
            .unwrap();
        let mut content = String::new();
        while let Some(result) = stream.next().await {
            let response = result.unwrap_or_else(|error| panic!("{api:?}: {error:?}"));
            assert!(response.error_message.is_none(), "{api:?}: {response:?}");
            if let Some(value) = response.content {
                for part in value.parts {
                    if let adk_core::Part::Text { text } = part {
                        content.push_str(&text);
                    }
                }
            }
        }
        assert!(content.contains(server::ANSWER), "{api:?}: {content}");
    }
}
