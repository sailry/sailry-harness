//! Official GPT-6 family defaults for standard, single-agent Responses requests.
use sailry_protocol::conversation::{ModelApi, Provider};

// https://developers.openai.com/api/docs/guides/reasoning#change-reasoning-mid-conversation
pub(crate) fn enable(provider: &mut Provider) {
    if provider.api != ModelApi::Responses {
        return;
    }
    for model in &mut provider.models {
        if super::search::supported(provider.api, &model.id)
            && model
                .id
                .strip_prefix("gpt-")
                .and_then(super::search::generation)
                .is_some_and(|(major, _)| major == 6)
        {
            model
                .capabilities
                .get_or_insert_default()
                .configuration_updates
                .get_or_insert(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn completes_only_supported_model_families() {
        let models = [
            "gpt-6.1-sol",
            "gpt-6-astra",
            "gpt-6.1-sol-20261001",
            "gpt-5.6-sol",
            "alias",
            "gpt-6invalid",
            "gpt-6-image",
            "gpt-6-audio",
            "gpt-7-sol",
        ]
        .map(|id| json!({"id":id,"context":200000,"output":16000}));
        let mut provider: Provider = serde_json::from_value(json!({
            "id":sailry_protocol::ProviderId::new(), "name":"Fixture", "api":"responses",
            "authentication":"api_key", "endpoint":"https://example.invalid/v1",
            "enabled":true, "revision":1, "credential":null, "default_model":"gpt-6.1-sol",
            "models":models
        }))
        .unwrap();
        enable(&mut provider);
        for (index, model) in provider.models.iter().enumerate() {
            assert_eq!(
                model
                    .capabilities
                    .as_ref()
                    .and_then(|value| value.configuration_updates),
                (index < 3).then_some(true)
            );
        }
        provider.models[0]
            .capabilities
            .as_mut()
            .unwrap()
            .configuration_updates = Some(false);
        enable(&mut provider);
        assert_eq!(
            provider.models[0]
                .capabilities
                .as_ref()
                .unwrap()
                .configuration_updates,
            Some(false)
        );
        provider.api = ModelApi::ChatCompletions;
        provider.models[1].capabilities = None;
        enable(&mut provider);
        assert!(provider.models[1].capabilities.is_none());
    }
}
