use super::*;
use sailry_protocol::SessionConfig;

#[tokio::test]
async fn foreign_models_keep_standard_chat_parameters() {
    for name in [
        "deepseek/deepseek-r1",
        "qwen/Qwen3-235B-A22B-Thinking",
        "z-ai/GLM-5",
        "moonshotai/kimi-k2.5",
        "anthropic/claude-sonnet-4-5",
        "minimax/minimax-m2.5",
        "unknown-reasoner",
        "custom/unknown-reasoner",
    ] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(ModelApi::ChatCompletions, server.endpoint.clone());
        let model = native(&provider, name, "key", HeaderMap::new());
        let mut input = request(name);
        input.reasoning = Some(true);
        input.temperature = Some(0.7);
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let sent = server.finish().await;
        assert_eq!(sent.len(), 1);
        let body = &sent[0].body;
        assert_eq!(body["messages"][0]["role"], "system", "{name}");
        assert_eq!(body["temperature"], 0.7, "{name}");
        assert_eq!(body["max_tokens"], 512, "{name}");
        assert!(body.get("max_completion_tokens").is_none(), "{name}");
    }
}

#[tokio::test]
async fn openai_policy_respects_capability_and_aliases() {
    for (name, capability, enabled) in [
        ("gpt-6-fixture", None, true),
        ("openai/o3", None, true),
        ("gpt-6-fixture", Some(false), false),
        ("gpt-4o", None, false),
        ("private-deployment", Some(true), false),
        ("private-deployment", None, false),
    ] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(ModelApi::ChatCompletions, server.endpoint.clone());
        let model = native(&provider, name, "key", HeaderMap::new());
        let mut input = request(name);
        input.reasoning = capability;
        input.temperature = Some(0.7);
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let sent = server.finish().await;
        assert_eq!(sent.len(), 1);
        let body = &sent[0].body;
        assert_eq!(
            body["messages"][0]["role"],
            if enabled { "developer" } else { "system" },
            "{name}/{capability:?}"
        );
        if enabled {
            assert!(body.get("temperature").is_none(), "{name}");
            assert_eq!(body["max_completion_tokens"], 512, "{name}");
            assert!(body.get("max_tokens").is_none(), "{name}");
        } else {
            assert_eq!(body["temperature"], 0.7, "{name}");
            assert_eq!(body["max_tokens"], 512, "{name}");
            assert!(body.get("max_completion_tokens").is_none(), "{name}");
        }
    }
}

#[tokio::test]
async fn budget_follows_configured_gateway() {
    for (api, suffix, name, responses) in [
        (ModelApi::OpenCodeGo, "/zen/go/v1", "glm-5", false),
        (ModelApi::OpenCodeZen, "/zen/v1", "deepseek-r1", false),
        (ModelApi::OpenCodeZen, "/v1", "gpt-6-fixture", true),
        (ModelApi::Responses, "/v1", "fixture", true),
        (ModelApi::ChatCompletions, "/v1", "gpt-6-fixture", false),
        (ModelApi::DeepSeek, "/v1", "deepseek-r1", false),
    ] {
        let server = Server::start(vec![if responses {
            fixtures::responses(false)
        } else {
            fixtures::chat()
        }])
        .await;
        let provider = provider(api, format!("{}{suffix}", server.endpoint));
        let session: SessionConfig = serde_json::from_value(json!({
            "provider":provider.id,"model":name,"effort":Effort::Budget(4096),
            "mode":"code","permission":"ask","credential":null
        }))
        .unwrap();
        let config = crate::agent::model::factory::config(&provider, &session, SessionId::new());
        let mut input = config.request(
            "Captured instructions".into(),
            vec![Message::user().with_text("Explain the result")],
            vec![],
        );
        input.reasoning = Some(true);
        input.max_tokens = Some(8192);
        let model = native(&provider, name, "key", HeaderMap::new());
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let sent = server.finish().await;
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0].body.get("budget_tokens"),
            matches!(api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen).then_some(&json!(4096)),
            "{api:?}"
        );
        assert_eq!(
            sent[0].path,
            format!(
                "{suffix}/{}",
                if responses {
                    "responses"
                } else {
                    "chat/completions"
                }
            )
        );
        if responses {
            assert_eq!(sent[0].body["max_output_tokens"], 8192);
        } else if name != "gpt-6-fixture" {
            assert_eq!(sent[0].body["messages"][0]["role"], "system");
            assert_eq!(sent[0].body["max_tokens"], 8192);
        }
        if api == ModelApi::OpenCodeGo {
            assert!(sent[0].headers.contains_key("x-opencode-session"));
        }
    }
}

#[tokio::test]
async fn responses_preserve_foreign_reasoning_replay() {
    for name in ["deepseek-r1", "custom/reasoner"] {
        let server = Server::start(vec![fixtures::responses(false)]).await;
        let provider = provider(ModelApi::Responses, server.endpoint.clone());
        let model = native(&provider, name, "key", HeaderMap::new());
        let mut input = request(name);
        input.reasoning = Some(true);
        input.temperature = Some(0.7);
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let sent = server.finish().await;
        assert_eq!(
            sent[0].body["include"],
            json!(["reasoning.encrypted_content"]),
            "{name}"
        );
        assert_eq!(sent[0].body["temperature"], 0.7, "{name}");
    }
}
