use super::*;
use core::prelude::v1::test;

fn provider(api: ModelApi, endpoint: &str) -> Provider {
    Provider {
        id: ProviderId::new(),
        revision: 1,
        name: "Provider".into(),
        api,
        authentication: sailry_protocol::Authentication::ApiKey,
        endpoint: endpoint.into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

#[test]
fn logos_follow_configured_type() {
    for (api, endpoint, expected) in [
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            Some("icons/providers/openai.svg"),
        ),
        (
            ModelApi::ChatCompletions,
            "https://api.x.ai/v1",
            Some("icons/providers/xai.svg"),
        ),
        (
            ModelApi::Anthropic,
            "https://proxy.example/v1",
            Some("icons/providers/anthropic.svg"),
        ),
        (
            ModelApi::AzureOpenAi,
            "https://example.openai.azure.com",
            None,
        ),
    ] {
        assert_eq!(logo(&provider(api, endpoint)).as_deref(), expected);
    }
}
