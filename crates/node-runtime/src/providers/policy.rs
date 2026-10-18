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

pub(crate) fn replay(provider: &Provider, model: &str) -> bool {
    policies(provider).iter().any(|policy| {
        policy.models.iter().any(|id| id == model) && policy.reasoning_replay.is_some()
    })
}

pub(crate) fn body(provider: &Provider, model: &str) -> Vec<Map<String, Value>> {
    policies(provider)
        .into_iter()
        .filter(|policy| policy.models.iter().any(|id| id == model) && !policy.body.is_empty())
        .map(|policy| policy.body)
        .collect()
}

pub(crate) fn merge(
    target: &mut Map<String, Value>,
    fields: &Map<String, Value>,
) -> Result<(), adk_core::AdkError> {
    for (key, value) in fields {
        if let Some(object) = value.as_object() {
            let target = target
                .entry(key)
                .or_insert_with(|| Value::Object(Map::new()));
            let target = target.as_object_mut().ok_or_else(|| {
                adk_core::AdkError::model("compatible thinking parameters must be an object")
            })?;
            merge(target, object)?;
        } else {
            target.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}
