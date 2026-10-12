use super::*;
use sailry_protocol::{
    conversation::connection::{Options, Product},
    usage::{Dimension, Query},
};

async fn select(fixture: &mut Fixture, model: &str, product: Product) {
    fixture.provider.models[0].id = model.into();
    fixture.provider.models[0].context = 65536;
    fixture.provider.models[0].output = 32768;
    fixture.provider.models[0].efforts = vec![Effort::Default];
    fixture.provider.models[0].default_effort = Effort::Default;
    fixture.provider.default_model = model.into();
    fixture.provider.options = Some(Options::Hosted { product });
    let Output::Provider(provider) = execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    fixture.provider = provider;
    let mut config = fixture.session.config.clone();
    config.model = model.into();
    config.effort = Effort::Default;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

#[tokio::test]
async fn preserves_regional_routes_and_usage() {
    use Product::{Coding, Metered};
    // Only each public endpoint's path is exercised. The isolated HTTP server
    // proves joining and Node transport behavior, not live provider acceptance.
    let routes = [
        (
            Metered,
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
        ),
        (
            Metered,
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
        ),
        (
            Coding,
            "https://coding.dashscope.aliyuncs.com/v1",
            "qwen3.6-plus",
        ),
        (
            Coding,
            "https://coding-intl.dashscope.aliyuncs.com/v1",
            "qwen3.6-plus",
        ),
        (Metered, "https://api.moonshot.cn/v1", "kimi-k2.6"),
        (Metered, "https://api.moonshot.ai/v1", "kimi-k2.6"),
        (Coding, "https://api.kimi.com/coding/v1", "kimi-for-coding"),
        (Coding, "https://api.kimi.ai/coding/v1", "kimi-for-coding"),
        (Metered, "https://api.minimax.cn/v1", "MiniMax-M3"),
        (Metered, "https://api.minimax.io/v1", "MiniMax-M3"),
        (
            Coding,
            "https://api.minimax.cn/v1",
            "MiniMax-M3.1-Flash-Preview",
        ),
        (
            Coding,
            "https://api.minimax.io/v1",
            "MiniMax-M3.1-Flash-Preview",
        ),
        (Metered, "https://open.bigmodel.cn/api/paas/v4", "glm-5.2"),
        (Metered, "https://api.z.ai/api/paas/v4", "glm-5.2"),
        (
            Coding,
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "glm-5.2",
        ),
        (Coding, "https://api.z.ai/api/coding/paas/v4", "glm-5.2"),
        (
            Metered,
            "https://ark.cn-beijing.volces.com/api/v3",
            "doubao-seed-2-0-pro-260215",
        ),
        (
            Coding,
            "https://ark.cn-beijing.volces.com/api/coding/v3",
            "ark-code-latest",
        ),
    ];
    for remote in [false, true] {
        for (product, public, model) in routes {
            let server = Server::start(ModelApi::ChatCompletions, Reply::Usage).await;
            let path = reqwest::Url::parse(public).unwrap().path().to_owned();
            let origin = server.endpoint.strip_suffix("/v1").unwrap();
            let endpoint = format!("{origin}{path}/");
            let mut fixture = Fixture::new(remote, ModelApi::ChatCompletions, &endpoint).await;
            select(&mut fixture, model, product).await;
            let turn = fixture.submit("Count the isolated response").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{public}, remote={remote}: {:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            let expected = Usage {
                input: 16,
                output: 4,
                cached_input: 6,
                reasoning: 1,
            };
            let reported: Vec<_> = page
                .entries
                .iter()
                .filter_map(|entry| entry.usage.clone())
                .collect();
            assert_eq!(
                reported,
                vec![expected.clone()],
                "{public}, remote={remote}"
            );
            let now = chrono::Utc::now().timestamp_millis();
            let usage = fixture
                .client
                .read_usage(Query {
                    start_ms: now - 86_400_000,
                    end_ms: now + 86_400_000,
                    dimension: Dimension::Model,
                    projects: vec![],
                    worktrees: vec![],
                    providers: vec![fixture.provider.id],
                    models: vec![model.into()],
                    before: None,
                })
                .await
                .unwrap();
            assert_eq!(usage.totals.responses, 1);
            assert_eq!(usage.totals.tokens, Some(expected));
            assert_eq!(usage.requests.items.len(), 1);
            assert_eq!(usage.requests.items[0].turn, turn);
            assert_eq!(usage.requests.items[0].session, fixture.session.id);
            assert_eq!(usage.requests.items[0].provider, fixture.provider.id);
            assert_eq!(usage.requests.items[0].model, model);
            assert_eq!(fixture.provider.options, Some(Options::Hosted { product }));
            assert!(!serde_json::to_string(&page).unwrap().contains(server::KEY));
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 1);
            let request = &requests[0];
            assert_eq!(request.path, format!("{path}/chat/completions"));
            assert_eq!(request.body["model"], model);
            assert_eq!(
                request.headers["authorization"],
                format!("Bearer {}", server::KEY)
            );
            assert_eq!(
                request.headers["user-agent"],
                concat!("Sailry/", env!("CARGO_PKG_VERSION"))
            );
            assert!(request.body["stream"].as_bool().unwrap());
            if model.starts_with("qwen3.6-plus") {
                assert_eq!(request.body["preserve_thinking"], true);
            } else if model == "glm-5.2" {
                assert_eq!(request.body["thinking"]["clear_thinking"], false);
                assert!(request.body["thinking"].get("type").is_none());
            } else {
                assert!(request.body.get("preserve_thinking").is_none());
                assert!(request.body.get("thinking").is_none());
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn replays_tool_continuations() {
    for remote in [false, true] {
        for (model, replay) in [
            ("kimi-for-coding", true),
            ("glm-5.2", true),
            ("qwen3.6-plus", true),
            ("MiniMax-M3.1-Flash-Preview", true),
            ("doubao-seed-2-0-pro-260215", true),
            ("ark-code-latest", false),
            ("fixture-a", false),
        ] {
            let server = Server::start(ModelApi::ChatCompletions, Reply::Tool).await;
            let mut fixture =
                Fixture::new(remote, ModelApi::ChatCompletions, &server.endpoint).await;
            select(&mut fixture, model, Product::Coding).await;
            std::fs::write(fixture.root.join("native.txt"), "工具内容 🙂").unwrap();
            let turn = fixture.submit("Read native.txt").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{model}, remote={remote}: {:?}",
                page.runs
            );
            assert!(text(&page).contains(server::ANSWER));
            assert!(
                page.entries.iter().flat_map(|entry| &entry.parts).any(
                    |part| matches!(part, Part::Thinking(text) if text == "Inspect the file 🙂")
                )
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2, "{model}, remote={remote}");
            let messages = requests[1].body["messages"].as_array().unwrap();
            let assistant = messages
                .iter()
                .find(|message| message["role"] == "assistant" && message["tool_calls"].is_array())
                .unwrap();
            assert_eq!(assistant["tool_calls"][0]["id"], "native-call");
            if replay {
                assert_eq!(
                    assistant["reasoning_content"], "Inspect the file 🙂",
                    "{model}, remote={remote}"
                );
            } else {
                assert!(
                    assistant.get("reasoning_content").is_none(),
                    "{model}, remote={remote}"
                );
            }
            let tool = messages
                .iter()
                .find(|message| message["role"] == "tool")
                .unwrap();
            assert_eq!(tool["tool_call_id"], "native-call");
            assert!(tool["content"].to_string().contains("工具内容 🙂"));
            for request in requests {
                assert_eq!(request.path, "/v1/chat/completions");
                assert_eq!(request.body["model"], model);
                assert_eq!(
                    request.headers["user-agent"],
                    concat!("Sailry/", env!("CARGO_PKG_VERSION"))
                );
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn keeps_anthropic_compatible_identity() {
    for remote in [false, true] {
        for (base, model) in [
            ("/anthropic", "MiniMax-M3.1-Flash-Preview"),
            ("/coding", "kimi-for-coding"),
            ("/api/coding", "ark-code-latest"),
            ("/apps/anthropic", "qwen3.6-plus"),
        ] {
            let server = Server::start(ModelApi::Anthropic, Reply::Usage).await;
            let endpoint = format!("{}{base}/v1/", server.endpoint);
            let mut fixture = Fixture::new(remote, ModelApi::Anthropic, &endpoint).await;
            select(&mut fixture, model, Product::Coding).await;
            let turn = fixture.submit("Count the compatible response").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{base}, remote={remote}: {:?}",
                page.runs
            );
            let reported: Vec<_> = page
                .entries
                .iter()
                .filter_map(|entry| entry.usage.clone())
                .collect();
            assert_eq!(
                reported,
                vec![Usage {
                    input: 16,
                    output: 4,
                    cached_input: 6,
                    reasoning: 0
                }]
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].path, format!("{base}/v1/messages"));
            assert_eq!(requests[0].body["model"], model);
            assert_eq!(requests[0].headers["x-api-key"], server::KEY);
            assert_eq!(
                requests[0].headers["user-agent"],
                concat!("Sailry/", env!("CARGO_PKG_VERSION"))
            );
            assert!(!requests[0].headers.contains_key("authorization"));
            assert!(requests[0].body.get("preserve_thinking").is_none());
            assert!(requests[0].body.get("thinking").is_none());
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
