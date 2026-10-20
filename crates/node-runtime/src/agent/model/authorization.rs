//! Refreshes inline provider grants at each ADK generation, including tool continuations.
use super::*;
use crate::providers::login::Grant;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, Part};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
mod claude;
mod codex;

// Adapters and observers consume the same immutable request capture, even when
// another helper refreshes the credential while this request is in flight.
#[derive(Clone)]
struct Scope {
    adapter: RequestAdapter,
    observer: Option<ResponseObserver>,
}
tokio::task_local! { static REQUEST: Scope; }

fn adapter(body: &mut Value, headers: &mut HeaderMap) -> Result<(), AdkError> {
    REQUEST
        .try_with(|scope| (scope.adapter)(body, headers))
        .map_err(|_| invalid())?
}
fn observe(headers: &HeaderMap) {
    let _ = REQUEST.try_with(|scope| {
        if let Some(observer) = &scope.observer {
            observer(headers);
        }
    });
}

pub(super) struct Model {
    pub(super) ingress: Arc<Ingress>,
    pub(super) provider: Provider,
    pub(super) config: SessionConfig,
    pub(super) session: sailry_protocol::SessionId,
    pub(super) delegated: bool,
    pub(super) effort: Effort,
    pub(super) stop: CancellationToken,
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        &self.config.model
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> Result<LlmResponseStream, AdkError> {
        let reference = self.config.credential.clone().ok_or_else(|| {
            failure(Fault::new(
                ErrorCode::NotConfigured,
                "provider sign-in is required",
            ))
        })?;
        let options = crate::providers::login::effective(&self.provider).map_err(failure)?;
        let authorized = self
            .ingress
            .authorization(
                reference.clone(),
                self.provider.id,
                self.provider.authentication,
                &options,
                &self.stop,
            )
            .await
            .map_err(failure)?;
        let mut provider = self.provider.clone();
        provider.endpoint = authorized.grant.endpoint().to_owned();
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = &self.ingress.authorization_endpoint {
            provider.endpoint = endpoint.clone();
        }
        let entry = pooled(
            &self.ingress.model_clients,
            &provider,
            &self.config,
            self.session,
            self.effort,
            &authorized,
        )
        .map_err(failure)?;
        let authorized = entry.authorize(authorized);
        let observer = matches!(&authorized.grant, Grant::ChatGpt { .. }).then(|| {
            self.ingress
                .quota_observer(self.provider.id, reference, authorized.credential.revision)
        });
        let grant = authorized.grant;
        let claude_tools = matches!(&grant, Grant::ClaudeOAuth { .. })
            .then(|| request.tools.keys().cloned().collect::<Vec<_>>());
        let base = grant.headers(&options).map_err(failure)?;
        let secrets = entry.protect(grant.secrets(&base));
        let (_, adapter, stream) = match grant {
            Grant::Plugin { record, .. } => {
                let adapter: RequestAdapter = Arc::new(move |_, headers| {
                    headers.extend(base.clone());
                    Ok(())
                });
                (record.issued.access, adapter, stream)
            }
            Grant::ChatGpt { access, .. } => {
                let adapter: RequestAdapter = Arc::new(move |body, headers| {
                    headers.extend(base.clone());
                    chatgpt(body)
                });
                (access, adapter, true)
            }
            Grant::ChatGptOAuth { access, .. } => {
                let session = self.session;
                let adapter: RequestAdapter = Arc::new(move |body, headers| {
                    headers.extend(base.clone());
                    codex::adapt(body, headers, session)
                });
                (access, adapter, true)
            }
            Grant::ClaudeOAuth { access, .. } => {
                let adapter: RequestAdapter =
                    Arc::new(move |body, headers| claude::adapt(body, headers, &base));
                (access, adapter, stream)
            }
            Grant::Copilot { access, .. } => {
                let initiator = if !self.delegated
                    && request.contents.last().is_some_and(|content| {
                        content.role == "user"
                            && !content
                                .parts
                                .iter()
                                .any(|part| matches!(part, Part::FunctionResponse { .. }))
                    }) {
                    "user"
                } else {
                    "agent"
                };
                let vision = request.contents.iter().flat_map(|content| &content.parts).any(|part| {
                    matches!(part, Part::InlineData { mime_type, .. } | Part::FileData { mime_type, .. } if mime_type.starts_with("image/"))
                });
                let adapter: RequestAdapter = Arc::new(move |_, headers| {
                    headers.extend(base.clone());
                    copilot(headers, initiator, vision)
                });
                (access, adapter, stream)
            }
        };
        let model = super::errors::wrap(entry.inner.clone(), secrets);
        let responses = scoped(Scope { adapter, observer }, model, request, stream).await?;
        Ok(match claude_tools {
            Some(tools) => claude::restore(responses, tools),
            None => responses,
        })
    }
}

fn pooled(
    registry: &registry::Registry,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    effort: Effort,
    authorization: &crate::store::login::Authorization,
) -> Result<Arc<registry::Entry>, Fault> {
    let adapted = matches!(
        provider.api,
        ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::Anthropic
    );
    let fingerprint = registry::fingerprint(
        provider,
        config,
        effort,
        (!adapted).then_some(authorization.credential.revision),
    )?;
    // These native SDKs accept authorization only in their constructor.
    let key = if adapted {
        "oauth-placeholder"
    } else {
        authorization.grant.access().expose()
    };
    registry.get(session, fingerprint, || {
        native(
            provider,
            config,
            key,
            effort,
            Some(Arc::new(adapter)),
            Some(Arc::new(observe)),
        )
    })
}

async fn scoped(
    scope: Scope,
    model: Arc<dyn Llm>,
    request: LlmRequest,
    stream: bool,
) -> Result<LlmResponseStream, AdkError> {
    let responses = REQUEST
        .scope(scope.clone(), model.generate_content(request, stream))
        .await?;
    Ok(Box::pin(futures::stream::unfold(
        (responses, scope),
        |(mut responses, scope)| async move {
            let response = REQUEST.scope(scope.clone(), responses.next()).await;
            response.map(|response| (response, (responses, scope)))
        },
    )))
}

fn chatgpt(body: &mut Value) -> Result<(), AdkError> {
    let fields = body.as_object_mut().ok_or_else(invalid)?;
    // Sign in with ChatGPT uses streamed, stateless public Responses requests.
    if fields.get("stream") != Some(&Value::Bool(true)) {
        return Err(invalid());
    }
    fields.insert("store".into(), Value::Bool(false));
    let input = fields
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(invalid)?;
    for item in input {
        if item["role"] == "system" {
            item.as_object_mut()
                .ok_or_else(invalid)?
                .insert("role".into(), json!("developer"));
        }
        if matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ) && item.get("namespace").is_none_or(Value::is_null)
        {
            item.as_object_mut()
                .ok_or_else(invalid)?
                .insert("namespace".into(), json!("sailry"));
        }
    }
    if let Some(tools) = fields.get_mut("tools") {
        namespace(tools.as_array_mut().ok_or_else(invalid)?)?;
    }
    let include = fields
        .entry("include")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(invalid)?;
    if !include
        .iter()
        .any(|value| value == "reasoning.encrypted_content")
    {
        include.push(json!("reasoning.encrypted_content"));
    }
    for key in [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        fields.remove(key);
    }
    Ok(())
}

fn namespace(tools: &mut Vec<Value>) -> Result<(), AdkError> {
    let mut local = Vec::new();
    for tool in std::mem::take(tools) {
        match tool.get("type").and_then(Value::as_str) {
            Some("function" | "custom") => local.push(tool),
            Some("namespace" | "web_search" | "web_search_preview") => tools.push(tool),
            _ => {
                return Err(AdkError::model(
                    "tool is not supported with ChatGPT plan use",
                ));
            }
        }
    }
    if !local.is_empty() {
        tools.push(json!({
            "type": "namespace",
            "name": "sailry",
            "description": "Sailry tools",
            "tools": local,
        }));
    }
    Ok(())
}

fn copilot(headers: &mut HeaderMap, initiator: &'static str, vision: bool) -> Result<(), AdkError> {
    for (name, value) in [
        ("openai-intent", "conversation-agent"),
        ("x-initiator", initiator),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    headers.insert(
        "x-request-id",
        HeaderValue::from_str(&sailry_protocol::RequestId::new().to_string())
            .map_err(|_| invalid())?,
    );
    if vision {
        headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
    }
    Ok(())
}

fn invalid() -> AdkError {
    AdkError::model("authorization request is invalid")
}
fn failure(fault: Fault) -> AdkError {
    AdkError::model("provider authorization failed").with_source(fault)
}

#[cfg(test)]
mod tests;
