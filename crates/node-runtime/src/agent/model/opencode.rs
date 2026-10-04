//! OpenCode delegates wire behavior to ADK's provider integration.
use super::*;
use adk_model::opencode::{OpenCodeApi, OpenCodeClient, OpenCodeConfig, OpenCodeService};
use sailry_protocol::SessionId;

fn service(api: ModelApi) -> Option<OpenCodeService> {
    match api {
        ModelApi::OpenCodeGo => Some(OpenCodeService::Go),
        ModelApi::OpenCodeZen => Some(OpenCodeService::Zen),
        _ => None,
    }
}

pub(super) fn api(api: ModelApi, model: &str) -> Result<ModelApi, Fault> {
    let Some(service) = service(api) else {
        return Ok(api);
    };
    Ok(match service.api(model) {
        Some(OpenCodeApi::ChatCompletions) => ModelApi::ChatCompletions,
        Some(OpenCodeApi::Responses) => ModelApi::Responses,
        Some(OpenCodeApi::Messages) => ModelApi::Anthropic,
        Some(OpenCodeApi::GenerateContent) => ModelApi::Gemini,
        None => {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "OpenCode model has no configured wire API",
            ));
        }
    })
}

pub(super) fn build(
    provider: &Provider,
    config: &SessionConfig,
    key: &str,
    effort: Effort,
    session: SessionId,
) -> Result<Arc<dyn Llm>, Fault> {
    let service = service(provider.api).expect("OpenCode protocol selected");
    let mut adapter = OpenCodeConfig::new(service, key, &config.model)
        .with_base_url(&provider.endpoint)
        .with_user_agent(concat!("Sailry/", env!("CARGO_PKG_VERSION")))
        .with_session_id(session.to_string())
        .with_retry_config(RetryConfig::disabled());
    if api(provider.api, &config.model)? == ModelApi::Anthropic {
        let native = reasoning::anthropic(AnthropicConfig::new(key, &config.model), effort)?;
        if let Some(thinking) = native.thinking {
            adapter = adapter.with_anthropic_thinking(thinking);
        }
        if let Some(effort) = native.effort {
            adapter = adapter.with_anthropic_effort(effort);
        }
    } else if api(provider.api, &config.model)? == ModelApi::Gemini {
        if let Some(thinking) = reasoning::gemini(effort)? {
            adapter = adapter.with_gemini_thinking(thinking);
        }
    } else if let Some(effort) = reasoning::openai(effort)? {
        adapter = adapter.with_reasoning_effort(effort);
    }
    Ok(Arc::new(OpenCodeClient::new(adapter).map_err(error)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_explicit_service_selection() {
        assert_eq!(
            api(ModelApi::Responses, "deepseek-v4.1-flash").unwrap(),
            ModelApi::Responses
        );
        assert_eq!(
            api(ModelApi::OpenCodeGo, "deepseek-v4.1-flash").unwrap(),
            ModelApi::ChatCompletions
        );
        assert_eq!(
            api(ModelApi::OpenCodeGo, "minimax-m3").unwrap(),
            ModelApi::Anthropic
        );
        assert_eq!(
            api(ModelApi::OpenCodeZen, "minimax-m3").unwrap(),
            ModelApi::ChatCompletions
        );
        assert_eq!(
            api(ModelApi::OpenCodeZen, "gemini-3.8-flash").unwrap(),
            ModelApi::Gemini
        );
        assert!(api(ModelApi::OpenCodeGo, "unknown-model").is_err());
        assert!(api(ModelApi::OpenCodeZen, "unknown-model").is_err());
        assert_eq!(
            api(ModelApi::ChatCompletions, "unknown-model").unwrap(),
            ModelApi::ChatCompletions
        );
    }
}
