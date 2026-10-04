use super::*;
use adk_model::{
    azure_ai::{AzureAIClient, AzureAIConfig},
    bedrock::{BedrockClient, BedrockConfig},
    openai::{AzureConfig, AzureOpenAIClient},
};
use sailry_protocol::conversation::cloud::Options;

fn bedrock(provider: &Provider, model: &str) -> Result<BedrockConfig, Fault> {
    let Some(Options::Bedrock { region }) = &provider.options else {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "Bedrock region is required",
        ));
    };
    Ok(BedrockConfig::new(region, model)
        .with_endpoint_url(&provider.endpoint)
        .without_prompt_caching())
}

fn vertex(
    provider: &Provider,
    model: &str,
    key: Option<&str>,
    effort: Effort,
) -> Result<GeminiModel, Fault> {
    let Some(Options::Vertex { project, location }) = &provider.options else {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "Vertex project and location are required",
        ));
    };
    let mut client =
        GeminiModel::new_google_cloud_endpoint(key, project, location, model, &provider.endpoint)
            .map_err(error)?
            .with_retry_config(RetryConfig::disabled());
    if let Some(thinking) = reasoning::gemini(effort)? {
        client = client.with_thinking_config(thinking);
    }
    Ok(client)
}

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
        ModelApi::Bedrock => Arc::new(
            BedrockClient::new_with_api_key(bedrock(provider, model)?, key)
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled()),
        ),
        ModelApi::Vertex => Arc::new(vertex(provider, model, Some(key), effort)?),
        _ => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "cloud API is required",
            ));
        }
    })
}

pub(super) async fn host(
    provider: &Provider,
    model: &str,
    effort: Effort,
) -> Result<Arc<dyn Llm>, Fault> {
    crate::providers::cloud::validate(provider)?;
    Ok(match provider.api {
        ModelApi::Bedrock => Arc::new(
            BedrockClient::new(bedrock(provider, model)?)
                .await
                .map_err(error)?
                .with_retry_config(RetryConfig::disabled()),
        ),
        ModelApi::Vertex => Arc::new(vertex(provider, model, None, effort)?),
        _ => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "provider does not support host identity",
            ));
        }
    })
}
