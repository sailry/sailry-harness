use super::*;
use serde_json::json;

fn grant(authentication: Authentication) -> login::Grant {
    let access = Secret::new("access".into());
    let refresh = Secret::new("refresh".into());
    let id_token = Secret::new("identity".into());
    match authentication {
        Authentication::ChatGpt => login::Grant::ChatGpt {
            access,
            refresh,
            id_token,
            expires_at_ms: 1,
            earliest_refresh_at: None,
            account_id: None,
            client_id: "oaiapp_fixture".into(),
            subject: "subject".into(),
            email: None,
            scopes: vec!["resource.invoke".into(), "chatgpt.tokens.use.direct".into()],
            renewal_pending: false,
        },
        Authentication::ChatGptOAuth => login::Grant::ChatGptOAuth {
            access,
            refresh,
            id_token,
            expires_at_ms: 1,
            subject: "subject".into(),
            account_id: "workspace".into(),
            email: None,
            renewal_pending: false,
        },
        _ => unreachable!(),
    }
}

#[test]
fn separates_catalog_request_identity() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::new();
    let legacy = grant(Authentication::ChatGptOAuth);
    let options = sailry_protocol::conversation::oauth::Options::ChatGptOAuth {
        catalog_version: Some("0.161.0".into()),
        user_agent: "CapturedAgent/1".into(),
    };
    let built = request(
        &client,
        endpoint_url(legacy.endpoint()).unwrap(),
        &legacy,
        &options,
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(
        built.url().as_str(),
        "https://chatgpt.com/backend-api/codex/models?client_version=0.161.0"
    );
    assert_eq!(built.headers()["authorization"], "Bearer access");
    assert_eq!(built.headers()["chatgpt-account-id"], "workspace");
    assert_eq!(built.headers()["originator"], "codex_cli_rs");
    assert_eq!(built.headers()["version"], "0.161.0");
    assert_eq!(built.headers()["user-agent"], "CapturedAgent/1");

    let siwc = grant(Authentication::ChatGpt);
    let retained = sailry_protocol::conversation::oauth::Options::ChatGpt {
        catalog_version: Some("0.161.0".into()),
        user_agent: "CapturedAgent/1".into(),
    };
    let built = request(
        &client,
        endpoint_url(siwc.endpoint()).unwrap(),
        &siwc,
        &retained,
    )
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(built.url().as_str(), "https://api.openai.com/v1/models");
    assert_eq!(built.headers()["authorization"], "Bearer access");
    assert_eq!(built.headers()["user-agent"], "CapturedAgent/1");
    for field in ["chatgpt-account-id", "originator", "version"] {
        assert!(!built.headers().contains_key(field));
    }
    assert!(
        request(
            &client,
            endpoint_url(siwc.endpoint()).unwrap(),
            &siwc,
            &options
        )
        .is_err()
    );
    assert!(
        request(
            &client,
            endpoint_url(legacy.endpoint()).unwrap(),
            &legacy,
            &retained
        )
        .is_err()
    );
}

#[test]
fn legacy_keeps_visible_native_metadata() {
    let value = json!({"models":[
        {"slug":"z-first","visibility":"list","display_name":"First","context_window":4096,"max_output_tokens":128,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
        {"slug":"hidden","visibility":"hide","context_window":false},
        {"slug":"a-second","visibility":"list","display_name":"Second"},
        {"slug":"a-second","visibility":"list","display_name":"Second"}
    ]});
    let models = parse(Authentication::ChatGptOAuth, ModelApi::Responses, &value).unwrap();
    assert_eq!(
        models,
        parse(Authentication::ChatGpt, ModelApi::Responses, &value).unwrap()
    );
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["z-first", "a-second"]
    );
    assert_eq!(models[0].capabilities.as_ref().unwrap().vision, Some(true));
    assert!(
        parse(
            Authentication::ChatGptOAuth,
            ModelApi::Responses,
            &json!({"data":[]})
        )
        .is_err()
    );
}

#[test]
fn advertises_only_reported_priority() {
    let mut value = json!({"models":[{"slug":"fixture","display_name":"Fixture","visibility":"list",
        "service_tiers":[{"id":"priority","name":"Priority"}],"default_service_tier":"priority"}]});
    let models = parse(Authentication::ChatGptOAuth, ModelApi::Responses, &value).unwrap();
    let choices = &models[0].capabilities.as_ref().unwrap().features;
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].id, "speed");
    assert_eq!(choices[0].options[0].id, "fast");
    assert!(choices[0].icon.as_ref().unwrap().valid());
    let configured =
        models[0].configuration(None, ModelApi::Responses, Authentication::ChatGptOAuth);
    assert_eq!(&configured.capabilities.as_ref().unwrap().features, choices);
    // An unrelated account catalog never borrows the Codex transport declaration.
    assert!(
        parse(Authentication::ChatGpt, ModelApi::Responses, &value).unwrap()[0]
            .capabilities
            .as_ref()
            .unwrap()
            .features
            .is_empty()
    );
    for tiers in [json!([]), json!([{"id":"standard"}]), json!(null)] {
        value["models"][0]["service_tiers"] = tiers;
        assert!(
            parse(Authentication::ChatGptOAuth, ModelApi::Responses, &value).unwrap()[0]
                .capabilities
                .as_ref()
                .unwrap()
                .features
                .is_empty()
        );
    }
    for tiers in [
        json!(false),
        json!([{"id":1}]),
        json!([{"id":"priority"},{"id":"priority"}]),
    ] {
        value["models"][0]["service_tiers"] = tiers;
        assert!(parse(Authentication::ChatGptOAuth, ModelApi::Responses, &value).is_err());
    }
}

#[test]
fn preserves_native_metadata() {
    let models = parse(Authentication::ChatGpt, ModelApi::Responses, &json!({"models":[
        {"slug":"known","visibility":"list","display_name":"Known model","context_window":4096,"max_output_tokens":128,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high"},
        {"slug":"future","visibility":"list","display_name":"Future model","supported_reasoning_levels":[{"effort":"new-native"}],"default_reasoning_level":"new-native"}
    ]})).unwrap();
    assert_eq!(models[0].id, "known");
    assert_eq!(models[0].display_name.as_deref(), Some("Known model"));
    assert_eq!(models[1].context, None);
    let future = models[1].capabilities.as_ref().unwrap();
    assert_eq!(future.reasoning, Some(true));
    assert_eq!(future.efforts, Some(vec![]));
    assert_eq!(future.default_effort, None);
    assert_eq!(models[0].context, Some(4096));
    let known = models[0].capabilities.as_ref().unwrap();
    assert_eq!(known.vision, Some(true));
    assert_eq!(known.tools, None);
    assert_eq!(known.efforts, Some(vec![Effort::Low, Effort::High]));
    assert_eq!(known.default_effort, Some(Effort::High));
}

#[test]
fn keeps_visible_order() {
    let models = parse(
        Authentication::ChatGpt,
        ModelApi::Responses,
        &json!({"models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"},
            {"slug":"hidden","visibility":"hide","context_window":false},
            {"slug":"a-second","display_name":"Second","visibility":"list"},
            {"slug":"unlisted"},
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"}
        ]}),
    )
    .unwrap();
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["gpt-6.1-sol", "a-second"]
    );
    assert_eq!(models[0].display_name.as_deref(), Some("GPT-6.1 Sol"));
    assert_eq!(models[0].context, None);
    assert_eq!(models[0].output, None);
    assert_eq!(models[0].capabilities.as_ref().unwrap().tools, None);
}

#[test]
fn leaves_unreported_limits_missing() {
    for (fields, expected) in [
        (json!({}), (None, None)),
        (
            json!({"context_window":null,"max_output_tokens":null}),
            (None, None),
        ),
        (json!({"context_window":272000}), (Some(272_000), None)),
        (json!({"max_output_tokens":128000}), (None, Some(128_000))),
    ] {
        let mut entry = json!({"slug":"fixture","display_name":"Fixture","visibility":"list"});
        entry
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let models = parse(
            Authentication::ChatGpt,
            ModelApi::Responses,
            &json!({"models":[entry]}),
        )
        .unwrap();
        assert_eq!((models[0].context, models[0].output), expected);
    }
}

#[test]
fn separates_copilot_surfaces() {
    let value = json!({"data":[
        {"id":"responses","capabilities":{"type":"chat","limits":{"max_context_window_tokens":8192,"max_prompt_tokens":4096,"max_output_tokens":1024},"supports":{"vision":false,"tool_calls":true,"reasoning_effort":["none","low","high"]}},"supported_endpoints":["/responses"]},
        {"id":"chat","capabilities":{"type":"chat"}},
        {"id":"embedding","capabilities":{"type":"embeddings"},"supported_endpoints":["/responses"]},
        {"id":"messages","capabilities":{"type":"chat"},"supported_endpoints":["/v1/messages"]}
    ]});
    let models = parse(Authentication::Copilot, ModelApi::Responses, &value).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "responses");
    assert_eq!(models[0].context, Some(8192));
    assert_eq!(models[0].output, Some(1024));
    let capabilities = models[0].capabilities.as_ref().unwrap();
    assert_eq!(capabilities.tools, Some(true));
    assert_eq!(capabilities.vision, Some(false));
    assert_eq!(
        capabilities.efforts,
        Some(vec![Effort::Disabled, Effort::Low, Effort::High])
    );
    let chat = parse(Authentication::Copilot, ModelApi::ChatCompletions, &value).unwrap();
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].id, "chat");
    assert_eq!(chat[0].context, None);
}

#[test]
fn rejects_conflicting_metadata() {
    for value in [
        json!({"models":[{"slug":"same","display_name":"Same","visibility":"list","context_window":2},{"slug":"same","display_name":"Same","visibility":"list","context_window":4}]}),
        json!({"models":[{"slug":"same","display_name":"First","visibility":"list"},{"slug":"same","display_name":"Second","visibility":"list"}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","context_window":2,"max_output_tokens":4}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"},{"effort":"high"}]}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"}],"default_reasoning_level":"low"}]}),
        json!({"models":[{"slug":"bad","display_name":"Bad","visibility":"list","input_modalities":[false]}]}),
        json!({"models":[{"slug":"bad","display_name":"","visibility":"list"}]}),
        json!({"models":[{"slug":"bad","visibility":"list"}]}),
    ] {
        assert!(parse(Authentication::ChatGpt, ModelApi::Responses, &value).is_err());
    }
    assert!(parse(Authentication::Copilot, ModelApi::Responses, &json!({"data":[{"id":"bad","capabilities":{"type":"chat","supports":{"vision":"true"}},"supported_endpoints":["/responses"]}]})).is_err());
}
