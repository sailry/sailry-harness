//! Request policy is immutable execution data, resolved before model construction.
use sailry_protocol::{
    Authentication, Effort,
    conversation::{ModelApi, Provider, presets::RequestPolicy},
};
use serde_json::{Map, Value};

pub(crate) fn policies(provider: &Provider) -> Vec<RequestPolicy> {
    if let Some(binding) = &provider.binding {
        return binding.policies.clone();
    }
    // Pre-extension captures and standalone native clients retain the original compatible policy.
    if provider.authentication == Authentication::ApiKey
        && provider.api == ModelApi::ChatCompletions
    {
        super::presets::bundled().policies
    } else {
        Vec::new()
    }
}

pub(crate) fn body(provider: &Provider, model: &str) -> Vec<Map<String, Value>> {
    policies(provider)
        .into_iter()
        .filter(|policy| policy.models.iter().any(|id| id == model) && !policy.body.is_empty())
        .map(|policy| policy.body)
        .collect()
}

pub(crate) fn reasoning(provider: &Provider, model: &str, effort: Effort) -> Map<String, Value> {
    if provider.api == ModelApi::Gemini {
        let level = match effort {
            Effort::Minimal => "minimal",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            _ => return Map::new(),
        };
        return [("thinking_level".into(), level.into())].into();
    }
    if !matches!(
        provider.api,
        ModelApi::Responses
            | ModelApi::ChatCompletions
            | ModelApi::DeepSeek
            | ModelApi::AzureOpenAi
    ) {
        return Map::new();
    }
    let level = match effort {
        Effort::Default | Effort::Budget(_) => return Map::new(),
        Effort::Disabled => "none",
        Effort::Minimal => "minimal",
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    };
    let mut fields = Map::new();
    if provider.api != ModelApi::DeepSeek || effort != Effort::Disabled {
        fields.insert("reasoning_effort".into(), level.into());
    }
    let thinking = body(provider, model)
        .into_iter()
        .filter_map(|mut fields| fields.remove("thinking"))
        .next_back();
    if provider.api == ModelApi::DeepSeek || thinking.is_some() {
        let mut thinking = thinking
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        thinking.insert(
            "type".into(),
            if effort == Effort::Disabled {
                "disabled"
            } else {
                "enabled"
            }
            .into(),
        );
        fields.insert("thinking".into(), Value::Object(thinking));
    }
    fields
}
