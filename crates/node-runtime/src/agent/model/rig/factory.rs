use super::super::types::{Error, ErrorKind};
use super::{cloud, transport::Transport};
use reqwest::header::{HeaderMap, HeaderValue};
use rig_core::{
    DynModel,
    operation::Completion,
    providers::{
        anthropic::AnthropicConfig,
        gemini::GeminiConfig,
        openai::{
            OpenAIConfig,
            responses_api::wire::Responses,
            wire::{Chat, DEEPSEEK, Dialect, OPENAI},
        },
    },
};
use sailry_protocol::{
    Authentication, SessionId,
    conversation::{ModelApi, Provider, connection::Options},
};

pub(super) struct Binding {
    pub model: DynModel<Completion>,
    pub api: ModelApi,
    pub underlying: String,
}

pub(super) async fn bind(
    client: reqwest::Client,
    provider: &Provider,
    model: &str,
    session: SessionId,
    key: &str,
    mut headers: HeaderMap,
) -> Result<Binding, Error> {
    if provider.authentication == Authentication::ChatGpt {
        return Err(Error::new(
            ErrorKind::NotConfigured,
            "this ChatGPT authorization endpoint is not supported",
        ));
    }
    let mut endpoint =
        crate::providers::endpoint_url(&provider.endpoint).map_err(super::errors::local)?;
    let mut api = provider.api;
    let mut underlying = model.to_owned();
    if api == ModelApi::AzureAi {
        crate::providers::cloud::validate(provider).map_err(super::errors::local)?;
        let route = cloud::resolve(&client, provider, model, key, &headers).await?;
        api = route.api;
        endpoint = route.endpoint;
        underlying = route.underlying;
    } else if api == ModelApi::AzureOpenAi {
        crate::providers::cloud::validate(provider).map_err(super::errors::local)?;
        let Some(Options::AzureOpenAi { api_version }) = &provider.options else {
            return Err(Error::new(
                ErrorKind::NotConfigured,
                "Azure API version is required",
            ));
        };
        let path = endpoint.path().trim_matches('/');
        let path = if path.contains("/deployments/") || path.starts_with("deployments/") {
            path.trim_end_matches("/chat/completions").to_owned()
        } else {
            let path = path.strip_suffix("/openai").unwrap_or(path);
            let path = if path == "openai" { "" } else { path };
            format!("{path}/openai/deployments/{model}")
        };
        endpoint.set_path(&format!("/{}", path.trim_start_matches('/')));
        cloud::replace_query(&mut endpoint, "api-version", api_version);
        api = ModelApi::ChatCompletions;
    }
    if matches!(api, ModelApi::OpenCodeGo | ModelApi::OpenCodeZen) {
        let path = gateway_path(endpoint.path());
        let response = path == "/v1" && cloud::responses_model(model)
            || endpoint
                .path()
                .trim_end_matches('/')
                .ends_with("/responses");
        endpoint.set_path(&path);
        if api == ModelApi::OpenCodeGo {
            headers.entry("x-opencode-session").or_insert(
                HeaderValue::from_str(&session.to_string()).map_err(|_| {
                    Error::new(ErrorKind::InvalidRequest, "session header is invalid")
                })?,
            );
        }
        api = if response {
            ModelApi::Responses
        } else {
            ModelApi::ChatCompletions
        };
    }
    let transport = Transport::new(
        client,
        provider,
        key,
        headers,
        &endpoint,
        api,
        (underlying != model).then(|| model.to_owned()),
    )
    .map_err(super::errors::local)?;
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    let base = endpoint.as_str().trim_end_matches('/');
    let bound = match api {
        ModelApi::Responses => {
            let base = base.strip_suffix("/responses").unwrap_or(base);
            let mut config = OpenAIConfig::new("").with_base_url(base);
            config.dialect.quirks.responses.reasoning_ciphertext = false;
            let wire = Responses::new(config, &underlying);
            rig_core::Model::new(wire, transport)
                .with_idle_timeout(None)
                .erase()
        }
        ModelApi::ChatCompletions | ModelApi::DeepSeek => {
            let base = base.strip_suffix("/chat/completions").unwrap_or(base);
            let dialect = if api == ModelApi::DeepSeek {
                DEEPSEEK
            } else {
                OPENAI
            };
            let mut dialect = if matches!(provider.api, ModelApi::AzureOpenAi | ModelApi::AzureAi) {
                Dialect::gateway("azure", "", "")
            } else {
                dialect
            };
            if matches!(
                provider.api,
                ModelApi::ChatCompletions
                    | ModelApi::DeepSeek
                    | ModelApi::OpenCodeGo
                    | ModelApi::OpenCodeZen
            ) {
                dialect.quirks.reasoning_field = Some("reasoning_content");
            }
            let config = OpenAIConfig::with_key(&dialect, "").with_base_url(base);
            rig_core::Model::new(Chat::new(config, &underlying), transport)
                .with_idle_timeout(None)
                .erase()
        }
        ModelApi::Anthropic => {
            let base = base.strip_suffix("/v1/messages").unwrap_or(base);
            let base = base.strip_suffix("/v1").unwrap_or(base);
            let mut wire = AnthropicConfig::new("")
                .with_base_url(base)
                .with_version("2023-06-01")
                .completion(&underlying)
                .with_prompt_caching();
            wire.tool_input_streaming =
                rig_core::providers::anthropic::wire::ToolInputStreaming::Off;
            rig_core::Model::new(super::anthropic::Messages(wire), transport)
                .with_idle_timeout(None)
                .erase()
        }
        ModelApi::Gemini => {
            if provider.authentication != Authentication::ApiKey
                || provider
                    .binding
                    .as_ref()
                    .and_then(|binding| binding.key_injection.as_ref())
                    .is_some_and(|injection| {
                        !injection.header.eq_ignore_ascii_case("x-goog-api-key")
                            || injection.template != "{key}"
                    })
            {
                return Err(Error::new(
                    ErrorKind::NotConfigured,
                    "Gemini requires API key authentication",
                ));
            }
            let base = base.strip_suffix("/v1beta").unwrap_or(base);
            GeminiConfig::new("")
                .with_base_url(base)
                .connect(transport)
                .completion(&underlying)
                .with_idle_timeout(None)
                .erase()
        }
        ModelApi::Plugin => {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                "plugin model requires its captured translation",
            ));
        }
        ModelApi::OpenCodeGo
        | ModelApi::OpenCodeZen
        | ModelApi::AzureOpenAi
        | ModelApi::AzureAi => {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                "model route was not resolved",
            ));
        }
    };
    Ok(Binding {
        model: bound,
        api,
        underlying,
    })
}

fn gateway_path(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if let Some(path) = path.strip_suffix("/chat/completions") {
        return path.to_owned();
    }
    if let Some(path) = path.strip_suffix("/responses") {
        return path.to_owned();
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    if last.strip_prefix('v').is_some_and(|version| {
        !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        path.to_owned()
    } else {
        format!("{path}/v1")
    }
}
