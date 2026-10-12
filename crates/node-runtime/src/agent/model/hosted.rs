//! API-key compatibility for documented assistant reasoning fields.
use adk_model::openai::RequestAdapter;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use sailry_protocol::{
    Authentication,
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
    policy(provider, model).is_some()
}

pub(super) fn adapter(provider: &Provider, model: &str) -> Option<RequestAdapter> {
    let policy = policy(provider, model)?;
    if matches!(policy, Policy::Replay) {
        return None;
    }
    Some(Arc::new(move |body, _| {
        let body = body.as_object_mut().ok_or_else(|| {
            adk_core::AdkError::model("compatible request body must be an object")
        })?;
        match policy {
            Policy::Qwen => {
                body.insert("preserve_thinking".into(), json!(true));
            }
            Policy::Glm => {
                let thinking = body.entry("thinking").or_insert_with(|| json!({}));
                let thinking = thinking.as_object_mut().ok_or_else(|| {
                    adk_core::AdkError::model("compatible thinking parameters must be an object")
                })?;
                thinking.insert("clear_thinking".into(), json!(false));
            }
            Policy::Replay => {}
        }
        Ok(())
    }))
}

#[derive(Clone, Copy)]
enum Policy {
    Replay,
    Qwen,
    Glm,
}

fn policy(provider: &Provider, model: &str) -> Option<Policy> {
    if provider.authentication != Authentication::ApiKey
        || provider.api != ModelApi::ChatCompletions
    {
        return None;
    }
    // Explicit model contracts, not product labels or generic reasoning metadata.
    // https://help.aliyun.com/en/model-studio/deep-thinking
    if matches!(model, "qwen3.6-plus" | "qwen3.6-plus-2026-04-02") {
        return Some(Policy::Qwen);
    }
    // https://docs.z.ai/guides/capabilities/thinking-mode
    if matches!(
        model,
        "glm-4.7" | "glm-5" | "glm-5.1" | "glm-5.2" | "glm-5.3" | "glm-5.3-flash"
    ) {
        return Some(Policy::Glm);
    }
    // https://platform.kimi.ai/docs/guide/use-thinking-models
    // https://www.kimi.com/en/help/kimi-code/error-reference
    // https://platform.minimax.io/docs/api-reference/text-chat-openai
    if matches!(
        model,
        "kimi-for-coding"
            | "kimi-k2.6"
            | "kimi-k2.7-code"
            | "kimi-k2.7-code-highspeed"
            | "kimi-k3"
            | "MiniMax-M3.1-Flash-Preview"
    ) {
        return Some(Policy::Replay);
    }
    // These Ark models return original plaintext reasoning. Newer encrypted
    // contracts and the dynamic ark-code-latest alias are not equivalent.
    // https://docs.volcengine.com/docs/ark/agent-model-invocation-practice?lang=zh
    matches!(
        model,
        "doubao-seed-2-0-pro-260215"
            | "doubao-seed-2-0-lite-260215"
            | "doubao-seed-2-0-mini-260215"
            | "deepseek-v4-pro-260425"
            | "deepseek-v4-flash-ga-260731"
            | "deepseek-v4-1-flash-260910"
            | "glm-5-2-260617"
    )
    .then_some(Policy::Replay)
}

#[cfg(test)]
mod tests;
