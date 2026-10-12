use super::*;
use adk_model::{
    azure_ai::{AzureAIClient, AzureAIConfig},
    openai::{AzureConfig, AzureOpenAIClient},
};
use sailry_protocol::conversation::connection::Options;

pub(super) fn keyed(
    provider: &Provider,
    model: &str,
    key: &str,
    effort: Effort,
) -> Result<Arc<dyn Llm>, Fault> {
    crate::providers::cloud::validate(provider)?;
    if key.is_empty() {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "cloud API key is required",
        ));
    }
    Ok(match provider.api {
        ModelApi::AzureOpenAi => {
            let Some(Options::AzureOpenAi { api_version }) = &provider.options else {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "Azure API version is required",
                ));
            };
            Arc::new(
                AzureOpenAIClient::new(AzureConfig::new(
                    key,
                    &provider.endpoint,
                    api_version,
                    model,
                ))
                .map_err(error)?
                .with_reasoning_effort(reasoning::openai(effort)?)
                .with_retry_config(RetryConfig::disabled()),
            )
        }
        ModelApi::AzureAi => Arc::new(
            AzureAIClient::new(AzureAIConfig::new(&provider.endpoint, key, model))
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled()),
        ),
        _ => {
            return Err(Fault::invalid("cloud API is required"));
        }
    })
}
