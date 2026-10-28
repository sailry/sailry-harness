//! Request policy is immutable execution data, resolved before model construction.
use sailry_protocol::{
    Authentication,
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
