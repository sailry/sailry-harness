//! GDK owns provider formats, HTTP streaming, and provider retry behavior.
use super::*;
use goose_provider_types::{
    base::Provider as GdkProvider, model::ModelConfig, thinking::ThinkingEffort,
};
use goose_providers::{
    anthropic::{ANTHROPIC_API_VERSION, AnthropicProviderBuilder},
    api_client::{ApiClient, AuthMethod, RequestBuilderDecorator},
    azure_foundry::AzureFoundryProvider,
    google::GoogleProvider,
    openai::OpenAiProviderBuilder,
    openai_compatible::OpenAiCompatibleProvider,
};
use reqwest::header::{HeaderMap, HeaderValue};
use sailry_protocol::{SessionId, conversation::connection::Options};
use serde_json::json;
use std::collections::HashMap;

pub(super) fn config(
    provider: &Provider,
    session: &SessionConfig,
    owner: SessionId,
) -> ModelConfig {
    let metadata = provider
        .models
        .iter()
        .find(|model| model.id == session.model);
    let mut config = ModelConfig::new(&session.model)
        .with_context_limit(
            metadata.and_then(|model| (model.context > 0).then_some(model.context as usize)),
        )
        .with_max_tokens(
            metadata.and_then(|model| i32::try_from(model.output).ok().filter(|value| *value > 0)),
        );
    if let Some(metadata) = metadata {
        config.reasoning = metadata
            .capabilities
            .as_ref()
            .and_then(|value| value.reasoning)
            .or(Some(metadata.reasoning));
        config.supports_vision = Some(metadata.vision);
    } else {
        config.supports_vision = Some(crate::providers::media::supports_api(
            sailry_protocol::media::Kind::Vision,
            provider,
        ));
    }
    let effort = if session.effort == Effort::Default {
        metadata.map_or(Effort::Default, |model| model.default_effort)
    } else {
        session.effort
    };
    config = match effort {
        Effort::Default => config,
        Effort::Disabled => config.with_thinking_effort(ThinkingEffort::Off),
        Effort::Minimal | Effort::Low => config.with_thinking_effort(ThinkingEffort::Low),
        Effort::Medium => config.with_thinking_effort(ThinkingEffort::Medium),
        Effort::High => config.with_thinking_effort(ThinkingEffort::High),
        Effort::XHigh | Effort::Max => config.with_thinking_effort(ThinkingEffort::Max),
        Effort::Budget(tokens) => config.with_merged_request_params(HashMap::from([(
            if provider.api == ModelApi::Gemini {
                "thinking_budget"
            } else {
                "budget_tokens"
            }
            .into(),
            json!(tokens),
        )])),
    };
    if provider.authentication == Authentication::ApiKey
        && matches!(
            provider.api,
            ModelApi::Responses | ModelApi::ChatCompletions
        )
    {
        config = config.with_merged_request_params(
            [("prompt_cache_key".into(), json!(owner.to_string()))].into(),
        );
    }
    // Captured package policies take precedence over these request defaults.
    for policy in crate::providers::policy::body(provider, &session.model) {
        config = config.with_merged_request_params(policy.into_iter().collect());
    }
    if metadata
        .and_then(|model| model.capabilities.as_ref())
        .and_then(|capabilities| capabilities.reasoning)
        != Some(false)
    {
        config = config.with_merged_request_params(
            crate::providers::policy::reasoning(provider, &session.model, effort)
                .into_iter()
                .collect(),
        );
    }
    if provider.api == ModelApi::DeepSeek && effort == Effort::Disabled {
        // The native API switches thinking off without an OpenAI effort field.
        config.reasoning = Some(false);
    }
    if matches!(provider.api, ModelApi::Anthropic | ModelApi::AzureAi)
        && effort == Effort::XHigh
        && goose_provider_types::formats::anthropic::thinking_type(&config)
            == goose_provider_types::formats::anthropic::ThinkingType::Adaptive
    {
        let mut output = config
            .request_param::<serde_json::Map<String, serde_json::Value>>("output_config")
            .unwrap_or_default();
        output.insert("effort".into(), json!("xhigh"));
        config =
            config.with_merged_request_params([("output_config".into(), json!(output))].into());
    }
    if provider.api == ModelApi::Responses
        && provider.authentication != Authentication::Copilot
        && metadata.is_some_and(|model| {
            model
                .validate_features(provider.api, &session.features)
                .is_ok()
        })
        && session
            .features
            .get("speed")
            .is_some_and(|value| value == "fast")
    {
        config =
            config.with_merged_request_params([("service_tier".into(), json!("priority"))].into());
    }
    config
}

pub(super) fn native(
    registry: &registry::Registry,
    provider: &Provider,
    model: &str,
    session: SessionId,
    key: &str,
    headers: HeaderMap,
) -> Result<Arc<dyn GdkProvider>, Fault> {
    let endpoint = crate::providers::endpoint_url(&provider.endpoint)?;
    let shared = registry.http(Some(endpoint.origin()))?;
    let mut definition = match provider.api {
        ModelApi::DeepSeek | ModelApi::OpenCodeGo | ModelApi::OpenCodeZen => {
            let name = match provider.api {
                ModelApi::DeepSeek => "custom_deepseek",
                ModelApi::OpenCodeGo => "opencode_go",
                _ => "opencode_zen",
            };
            Some(
                goose_providers::declarative::fixed_provider_configs()
                    .map_err(unavailable)?
                    .into_iter()
                    .find(|config| config.name == name)
                    .ok_or_else(|| {
                        Fault::new(
                            ErrorCode::NotConfigured,
                            "GDK provider definition is unavailable",
                        )
                    })?,
            )
        }
        _ => None,
    };
    let mut static_headers = headers;
    if let Some(definition) = &definition {
        for (name, value) in definition.headers.iter().flatten() {
            let name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(unavailable)?;
            static_headers
                .entry(name)
                .or_insert(HeaderValue::from_str(value).map_err(unavailable)?);
        }
        if let Some(name) = &definition.session_id_header_override {
            let name =
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(unavailable)?;
            static_headers
                .entry(name)
                .or_insert(HeaderValue::from_str(&session.to_string()).map_err(unavailable)?);
        }
    }
    static_headers
        .entry(reqwest::header::USER_AGENT)
        .or_insert(HeaderValue::from_static(concat!(
            "Sailry/",
            env!("CARGO_PKG_VERSION")
        )));
    static_headers
        .entry(reqwest::header::CONTENT_TYPE)
        .or_insert(HeaderValue::from_static("application/json"));
    if matches!(provider.api, ModelApi::Anthropic | ModelApi::AzureAi) {
        static_headers
            .entry(reqwest::header::HeaderName::from_static(
                "anthropic-version",
            ))
            .or_insert(HeaderValue::from_static(ANTHROPIC_API_VERSION));
    }
    let decorator: RequestBuilderDecorator = Arc::new(move |request| {
        // ApiClient applies auth after this decorator; keep it on the rebuilt request.
        // Its private default headers are not carried by RequestBuilder::build().
        let mut request = request.build()?;
        for (name, value) in &static_headers {
            if !request.headers().contains_key(name) {
                request.headers_mut().insert(name.clone(), value.clone());
            }
        }
        Ok(reqwest::RequestBuilder::from_parts(shared.clone(), request))
    });
    let injection = provider
        .binding
        .as_ref()
        .and_then(|binding| binding.key_injection.as_ref());
    let auth = || match crate::providers::request::auth(provider, key) {
        crate::providers::request::KeyAuth::None => AuthMethod::NoAuth,
        crate::providers::request::KeyAuth::Bearer(value) => AuthMethod::BearerToken(value),
        crate::providers::request::KeyAuth::Header { name, value } => AuthMethod::ApiKey {
            header_name: name,
            key: value,
        },
    };
    let client = |host: String, query: Vec<(String, String)>| -> Result<ApiClient, Fault> {
        let client = ApiClient::new_with_tls(host, auth(), None)
            .map_err(unavailable)?
            .with_query(query)
            .with_request_builder(decorator.clone())
            .with_same_origin_redirects()
            .map_err(unavailable)?;
        Ok(client)
    };
    let query = endpoint
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let authority = endpoint[..url::Position::BeforePath].to_owned();
    let path = endpoint.path().trim_matches('/');
    let inner: Arc<dyn GdkProvider> = match provider.api {
        ModelApi::Responses => {
            let path: String = if path.ends_with("responses") {
                path.into()
            } else {
                format!("{}/responses", path.trim_end_matches('/'))
                    .trim_start_matches('/')
                    .into()
            };
            Arc::new(
                OpenAiProviderBuilder::new(client(authority, query)?)
                    .base_path(path)
                    .session_id_provider(Arc::new(move || Some(session.to_string())))
                    .build(),
            )
        }
        ModelApi::ChatCompletions => {
            Arc::new(
                OpenAiProviderBuilder::new(client(authority, query)?)
                    // A leading slash makes the configured Chat route explicit in GDK.
                    .base_path(completions(path))
                    .preserve_thinking_context(true)
                    .build(),
            )
        }
        ModelApi::Anthropic => {
            let path = match path {
                "v1" | "v1/messages" => "",
                _ => path
                    .strip_suffix("/v1/messages")
                    .or_else(|| path.strip_suffix("/v1"))
                    .unwrap_or(path),
            };
            let host = if path.is_empty() {
                authority
            } else {
                format!("{authority}/{path}")
            };
            Arc::new(AnthropicProviderBuilder::new(client(host, query)?).build())
        }
        ModelApi::Gemini => {
            if provider.authentication != Authentication::ApiKey
                || injection.is_some_and(|injection| {
                    !injection.header.eq_ignore_ascii_case("x-goog-api-key")
                        || injection.template != "{key}"
                })
            {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "GDK Gemini requires API key authentication",
                ));
            }
            let path = path
                .strip_suffix("/v1beta")
                .or_else(|| path.strip_suffix("v1beta"))
                .unwrap_or(path)
                .trim_end_matches('/');
            let host = if path.is_empty() {
                authority
            } else {
                format!("{authority}/{path}")
            };
            // GoogleProvider owns x-goog-api-key; arbitrary endpoint queries stay on its decorator.
            let mut decorator = decorator;
            if !query.is_empty() {
                let previous = decorator;
                decorator = Arc::new(move |request| {
                    let request = previous(request)?;
                    Ok(request.query(&query))
                });
            }
            Arc::new(
                GoogleProvider::new(host, key.into(), None, Some(decorator), None)
                    .map_err(unavailable)?,
            )
        }
        ModelApi::AzureOpenAi => {
            crate::providers::cloud::validate(provider)?;
            let Some(Options::AzureOpenAi { api_version }) = &provider.options else {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "Azure API version is required",
                ));
            };
            let mut query = query;
            query.retain(|(name, _)| name != "api-version");
            query.push(("api-version".into(), api_version.clone()));
            let base = path.trim_end_matches('/');
            let prefix = if base.contains("/deployments/") || base.starts_with("deployments/") {
                format!("{}/", base.trim_end_matches("/chat/completions"))
            } else {
                let base = base.strip_suffix("/openai").unwrap_or(base);
                let base = if base == "openai" { "" } else { base };
                format!("{base}/openai/deployments/{model}/")
                    .trim_start_matches('/')
                    .into()
            };
            Arc::new(OpenAiCompatibleProvider::new(
                "azure_openai".into(),
                client(authority, query)?,
                prefix,
            ))
        }
        ModelApi::AzureAi => {
            crate::providers::cloud::validate(provider)?;
            let maas = goose_providers::azure_foundry::endpoint_kind(&provider.endpoint)
                == goose_providers::azure_foundry::EndpointKind::Maas;
            let (host, decorator) = if maas {
                let path = path.trim_end_matches("/chat/completions");
                let path = path.strip_suffix("/v1").unwrap_or(path);
                let path = if path == "v1" { "" } else { path };
                let host = if path.is_empty() {
                    authority
                } else {
                    format!("{authority}/{path}")
                };
                let query: RequestBuilderDecorator = Arc::new(move |request| {
                    Ok(decorator(request)?.query(&[("api-version", "2024-05-01-preview")]))
                });
                (host, query)
            } else {
                (provider.endpoint.clone(), decorator)
            };
            Arc::new(
                AzureFoundryProvider::create(
                    host,
                    None,
                    Some(model.into()),
                    auth(),
                    auth(),
                    auth(),
                    auth(),
                    None,
                    Some(decorator),
                )
                .map_err(unavailable)?,
            )
        }
        ModelApi::DeepSeek | ModelApi::OpenCodeGo | ModelApi::OpenCodeZen => {
            let mut definition = definition.take().unwrap();
            definition.base_url = provider.endpoint.clone();
            let captured_key = key.to_owned();
            let mut builder = goose_providers::openai::from_declarative_config(
                definition,
                None,
                CapturedKey(captured_key),
            )
            .map_err(unavailable)?;
            if provider.api == ModelApi::DeepSeek {
                builder = builder.base_path(completions(path));
            }
            Arc::new(
                builder
                    .try_map_api_client(|client| {
                        Ok(client.with_auth(auth()).with_request_builder(decorator))
                    })
                    .map_err(unavailable)?
                    .session_id_provider(Arc::new(move || Some(session.to_string())))
                    .build(),
            )
        }
        ModelApi::Plugin => {
            return Err(Fault::invalid(
                "plugin model requires its captured translation",
            ));
        }
    };
    Ok(inner)
}

fn completions(path: &str) -> String {
    if path.ends_with("chat/completions") {
        format!("/{path}")
    } else if path.is_empty() {
        "/chat/completions".into()
    } else {
        format!("/{path}/chat/completions")
    }
}

struct CapturedKey(String);
impl goose_providers::declarative::KeyResolver for CapturedKey {
    type Error = std::convert::Infallible;
    fn resolve_key(&self, _key: &str) -> Result<String, Self::Error> {
        Ok(self.0.clone())
    }
}

pub(super) fn unavailable(error: impl std::fmt::Display) -> Fault {
    let _ = error;
    Fault::new(
        ErrorCode::Unavailable,
        "model provider could not be configured",
    )
}

#[cfg(test)]
mod tests;
