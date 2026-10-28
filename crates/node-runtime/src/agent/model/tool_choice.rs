//! Summary requests preserve declarations while asking providers for text only.
use adk_core::LlmRequest;
use adk_model::openai::RequestAdapter;
use sailry_protocol::conversation::ModelApi;
use serde_json::json;
use std::sync::Arc;

pub(super) fn adapter(api: ModelApi, inner: Option<RequestAdapter>) -> Option<RequestAdapter> {
    if !matches!(
        api,
        ModelApi::Responses
            | ModelApi::ChatCompletions
            | ModelApi::Anthropic
            | ModelApi::DeepSeek
            | ModelApi::AzureOpenAi
            | ModelApi::AzureAi
    ) {
        return inner;
    }
    Some(Arc::new(move |body, headers| {
        if let Some(inner) = &inner {
            inner(body, headers)?;
        }
        if super::call::is_summary()
            && body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
        {
            body["tool_choice"] = if api == ModelApi::Anthropic {
                json!({"type":"none"})
            } else {
                json!("none")
            };
        }
        Ok(())
    }))
}

pub(super) fn prepare(request: &mut LlmRequest, provider: &str) {
    if !super::call::is_summary() {
        return;
    }
    let api = match provider {
        "gemini" => ModelApi::Gemini,
        "open_code_go" => ModelApi::OpenCodeGo,
        "open_code_zen" => ModelApi::OpenCodeZen,
        _ => return,
    };
    if !matches!(
        super::opencode::api(api, &request.model),
        Ok(ModelApi::Gemini)
    ) {
        return;
    }
    // ADK consumes this declaration extension into top-level toolConfig while
    // leaving the actual function declarations unchanged.
    for declaration in request.tools.values_mut() {
        declaration["x-adk-gemini-tool-config"]["function_calling_config"]["mode"] = json!("NONE");
    }
}

#[cfg(test)]
mod tests;
