use super::*;
use crate::agent::model::{self, reasoning};
use adk_core::{Content, LlmRequest, ProviderOrigin};
use futures::StreamExt;
use sailry_protocol::{
    Authentication, Effort, Permission, ProviderId, SessionConfig, WorkMode, conversation::Provider,
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn disables_tools_in_actual_requests() {
    for (api, model_name) in [
        (ModelApi::Responses, "fixture"),
        (ModelApi::ChatCompletions, "fixture"),
        (ModelApi::Anthropic, "fixture"),
        (ModelApi::Gemini, "fixture"),
        (ModelApi::DeepSeek, "fixture"),
        (ModelApi::AzureAi, "fixture"),
        (ModelApi::AzureOpenAi, "fixture"),
        (ModelApi::OpenCodeGo, "deepseek-v4.1-flash"),
        (ModelApi::OpenCodeGo, "minimax-m3"),
        (ModelApi::OpenCodeGo, "gpt-6-luna"),
        (ModelApi::OpenCodeZen, "gemini-3.8-flash"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}{}",
            listener.local_addr().unwrap(),
            if matches!(api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
                "/v1"
            } else {
                ""
            }
        );
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for _ in 0..6 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let body = loop {
                    let mut chunk = [0; 8192];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                    let Some(header) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                        continue;
                    };
                    let length = std::str::from_utf8(&bytes[..header])
                        .unwrap()
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        })
                        .unwrap()
                        .1
                        .trim()
                        .parse::<usize>()
                        .unwrap();
                    if bytes.len() >= header + 4 + length {
                        break serde_json::from_slice::<Value>(
                            &bytes[header + 4..header + 4 + length],
                        )
                        .unwrap();
                    }
                };
                requests.push(body);
                let body = r#"{"error":{"message":"fixture request captured","type":"invalid_request_error","code":"fixture"}}"#;
                stream.write_all(format!("HTTP/1.1 400 Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            requests
        });
        let provider = Provider {
            id: ProviderId::new(),
            revision: 1,
            preset: None,
            binding: None,
            consent: None,
            name: "Summary fixture".into(),
            api,
            authentication: Authentication::ApiKey,
            endpoint,
            options: (api == ModelApi::AzureOpenAi).then(|| {
                sailry_protocol::conversation::connection::Options::AzureOpenAi {
                    api_version: "2024-12-01-preview".into(),
                }
            }),
            oauth: None,
            enabled: true,
            models: vec![],
            default_model: "fixture".into(),
            request_max_retries: None,
            stream_max_retries: None,
            credential: None,
        };
        let wire = model::opencode::api(api, model_name).unwrap();
        let config = SessionConfig {
            provider: provider.id,
            model: model_name.into(),
            credential: None,
            assistant: None,
            resource: None,
            effort: Effort::Default,
            features: Default::default(),
            options: Default::default(),
            mode: WorkMode::Code,
            permission: Permission::Ask,
        };
        let native = if matches!(api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
            model::opencode::build(
                &provider,
                &config,
                "fixture-key",
                Effort::Default,
                sailry_protocol::SessionId::new(),
            )
            .unwrap()
        } else {
            model::native(
                &provider,
                &config,
                "fixture-key",
                reasoning::Resolved {
                    effort: Effort::Default,
                    summary: false,
                    encrypted: false,
                },
                None,
                None,
                None,
            )
            .unwrap()
            .inner
        };
        let origin = ProviderOrigin {
            provider: serde_json::to_value(api).unwrap().as_str().unwrap().into(),
            model: model_name.into(),
            scope: "fixture".into(),
        };
        let mut input = LlmRequest::new(
            model_name,
            vec![Content::new("user").with_text("Return a factual handoff")],
        );
        input.tools.insert("lookup".into(), json!({"name":"lookup","description":"Look up facts","parameters":{"type":"object","properties":{}}}));
        for declared in [true, false] {
            if !declared {
                input.tools.clear();
            }
            for summary in [false, true, false] {
                let send = async {
                    let response = model::history::generate(
                        input.clone(),
                        origin.clone(),
                        None,
                        |request| async { native.generate_content(request, false).await },
                    )
                    .await;
                    match response {
                        Ok(mut stream) => assert!(stream.next().await.unwrap().is_err()),
                        Err(_) => {}
                    }
                };
                if summary {
                    model::summary_scope(send).await;
                } else {
                    send.await;
                }
            }
        }
        let requests = tokio::time::timeout(std::time::Duration::from_secs(10), server)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(requests.len(), 6);
        for (declared, requests) in [true, false].into_iter().zip(requests.chunks_exact(3)) {
            if !declared {
                assert!(
                    requests
                        .iter()
                        .all(|request| request["tools"].as_array().is_none_or(Vec::is_empty))
                );
                assert!(
                    requests
                        .iter()
                        .all(|request| request.get("toolConfig").is_none())
                );
                assert_eq!(requests[0]["tool_choice"], requests[1]["tool_choice"]);
                assert_eq!(requests[0]["tool_choice"], requests[2]["tool_choice"]);
                assert_ne!(requests[1]["tool_choice"], json!("none"));
                assert_ne!(requests[1]["tool_choice"], json!({"type":"none"}));
                continue;
            }
            assert!(!requests[0]["tools"].as_array().unwrap().is_empty());
            assert_eq!(requests[0]["tools"], requests[1]["tools"]);
            assert_eq!(requests[0]["tools"], requests[2]["tools"]);
            if wire == ModelApi::Gemini {
                assert_eq!(
                    requests[1]["toolConfig"]["function_calling_config"]["mode"],
                    "NONE"
                );
                assert!(requests[0].get("toolConfig").is_none());
                assert!(requests[2].get("toolConfig").is_none());
            } else {
                assert_eq!(
                    requests[1]["tool_choice"],
                    if wire == ModelApi::Anthropic {
                        json!({"type":"none"})
                    } else {
                        json!("none")
                    }
                );
                assert_ne!(requests[0]["tool_choice"], requests[1]["tool_choice"]);
                assert_eq!(requests[0]["tool_choice"], requests[2]["tool_choice"]);
            }
        }
    }
}
