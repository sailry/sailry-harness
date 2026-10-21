//! API-key cache identity and documented assistant reasoning fields.
use adk_model::openai::RequestAdapter;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use sailry_protocol::{
    Authentication, SessionId,
    conversation::{ModelApi, Provider},
};
use serde_json::json;
use std::sync::Arc;

pub(super) fn headers() -> HeaderMap {
    HeaderMap::from_iter([(
        USER_AGENT,
        HeaderValue::from_static(concat!("Sailry/", env!("CARGO_PKG_VERSION"))),
    )])
}

pub(super) fn replay(provider: &Provider, model: &str) -> bool {
    crate::providers::policy::replay(provider, model)
}

pub(super) fn session_adapter(
    provider: &Provider,
    model: &str,
    session: SessionId,
    encrypted: bool,
) -> Option<RequestAdapter> {
    if provider.authentication != Authentication::ApiKey
        || !matches!(
            provider.api,
            ModelApi::Responses | ModelApi::ChatCompletions
        )
    {
        return None;
    }
    let reasoning = adapter(provider, model);
    let responses = provider.api == ModelApi::Responses;
    let session = session.to_string();
    Some(Arc::new(move |body, headers| {
        if let Some(adapter) = &reasoning {
            adapter(body, headers)?;
        }
        if responses {
            stateless(body, encrypted)?;
        }
        let fields = body.as_object_mut().ok_or_else(|| {
            adk_core::AdkError::model("compatible request body must be an object")
        })?;
        fields.insert("prompt_cache_key".into(), json!(session));
        let value = HeaderValue::from_str(&session)
            .map_err(|_| adk_core::AdkError::model("session ID is not a valid HTTP header"))?;
        headers.insert("session-id", value.clone());
        headers.insert("session_id", value);
        Ok(())
    }))
}

pub(super) fn stateless(
    body: &mut serde_json::Value,
    encrypted: bool,
) -> Result<(), adk_core::AdkError> {
    let fields = body
        .as_object_mut()
        .ok_or_else(|| adk_core::AdkError::model("Responses request body must be an object"))?;
    fields.remove("previous_response_id");
    fields.insert("store".into(), json!(false));
    if let Some(input) = fields
        .get_mut("input")
        .and_then(serde_json::Value::as_array_mut)
    {
        // Stored histories keep their original items; stateless requests can replay
        // reasoning only when the provider returned its opaque encrypted content.
        input.retain(|item| {
            item["type"] != "reasoning"
                || item["encrypted_content"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
        });
    }
    if encrypted {
        let include = fields
            .entry("include")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| adk_core::AdkError::model("Responses include must be an array"))?;
        if !include
            .iter()
            .any(|item| item == "reasoning.encrypted_content")
        {
            include.push(json!("reasoning.encrypted_content"));
        }
    }
    Ok(())
}

pub(super) fn adapter(provider: &Provider, model: &str) -> Option<RequestAdapter> {
    let policies = crate::providers::policy::body(provider, model);
    if policies.is_empty() {
        return None;
    }
    Some(Arc::new(move |body, _| {
        let body = body.as_object_mut().ok_or_else(|| {
            adk_core::AdkError::model("compatible request body must be an object")
        })?;
        for policy in &policies {
            crate::providers::policy::merge(body, policy)?;
        }
        Ok(())
    }))
}

#[cfg(test)]
mod tests;
