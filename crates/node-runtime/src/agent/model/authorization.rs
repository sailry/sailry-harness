//! Refreshes inline provider grants at each ADK generation, including tool continuations.
use super::*;
use crate::providers::login::Grant;
use adk_core::{AdkError, LlmRequest, LlmResponseStream, Part, SchemaAdapter};
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

pub(super) struct Context {
    pub session: sailry_protocol::SessionId,
    pub delegated: bool,
    pub reasoning: reasoning::Resolved,
    pub purpose: Purpose,
    pub stop: CancellationToken,
}

pub(super) struct Model {
    ingress: Arc<Ingress>,
    provider: Provider,
    config: SessionConfig,
    context: Context,
    metadata: Arc<registry::Entry>,
}

struct Prepared {
    entry: Arc<registry::Entry>,
    scope: Scope,
    secrets: Vec<sailry_protocol::Secret>,
    tools: Option<Vec<String>>,
    stream: bool,
    origin: adk_core::ProviderOrigin,
}

impl Model {
    pub(super) async fn new(
        ingress: &Arc<Ingress>,
        provider: &Provider,
        config: &SessionConfig,
        context: Context,
    ) -> Result<Self, Fault> {
        let (effective, authorized) = current(ingress, provider, config, &context.stop).await?;
        let metadata = pooled(
            &ingress.model_clients,
            &effective,
            config,
            context.session,
            context.reasoning,
            context.purpose,
            &authorized,
        )?;
        metadata.authorize(authorized);
        Ok(Self {
            ingress: ingress.clone(),
            provider: provider.clone(),
            config: config.clone(),
            context,
            metadata,
        })
    }

    async fn prepare(&self, request: &LlmRequest, stream: bool) -> Result<Prepared, AdkError> {
        let (provider, authorized) = current(
            &self.ingress,
            &self.provider,
            &self.config,
            &self.context.stop,
        )
        .await
        .map_err(failure)?;
        let reference = self
            .config
            .credential
            .clone()
            .expect("authorization requires a credential");
        let options = crate::providers::login::effective(&self.provider).map_err(failure)?;
        let entry = pooled(
            &self.ingress.model_clients,
            &provider,
            &self.config,
            self.context.session,
            self.context.reasoning,
            self.context.purpose,
            &authorized,
        )
        .map_err(failure)?;
        let authorized = entry.authorize(authorized);
        let origin = super::history::origin(
            &provider,
            &self.config.model,
            Some(&authorized.credential),
            Some(&authorized.grant),
        );
        let observer = matches!(&authorized.grant, Grant::ChatGpt { .. }).then(|| {
            self.ingress
                .quota_observer(self.provider.id, reference, authorized.credential.revision)
        });
        let grant = authorized.grant;
        let claude_tools = matches!(&grant, Grant::ClaudeOAuth { .. })
            .then(|| request.tools.keys().cloned().collect::<Vec<_>>());
        let base = grant.headers(&options).map_err(failure)?;
        let secrets = entry.protect(grant.secrets(&base));
        let (adapter, stream) = match grant {
            Grant::Plugin { .. } => {
                let adapter: RequestAdapter = Arc::new(move |_, headers| {
                    headers.remove("authorization");
                    headers.remove("x-api-key");
                    headers.extend(base.clone());
                    Ok(())
                });
                (adapter, stream)
            }
            Grant::ChatGpt { .. } => {
                let adapter: RequestAdapter = Arc::new(move |body, headers| {
                    headers.extend(base.clone());
                    chatgpt(body)
                });
                (adapter, true)
            }
            Grant::ChatGptOAuth { .. } => {
                let session = self.context.session;
                let adapter: RequestAdapter = Arc::new(move |body, headers| {
                    headers.extend(base.clone());
                    codex::adapt(body, headers, session)
                });
                (adapter, true)
            }
            Grant::ClaudeOAuth { .. } => {
                let adapter: RequestAdapter =
                    Arc::new(move |body, headers| claude::adapt(body, headers, &base));
                (adapter, stream)
            }
            Grant::Copilot { .. } => {
                let initiator = if !self.context.delegated
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
                (adapter, stream)
            }
        };
        let adapter = captured(
            &self.provider,
            &self.config.model,
            adapter,
            self.context.reasoning.encrypted,
        );
        Ok(Prepared {
            entry,
            scope: Scope { adapter, observer },
            secrets,
            tools: claude_tools,
            stream,
            origin,
        })
    }

    pub(super) async fn preconnect(&self) -> Result<(), AdkError> {
        if self.provider.api != ModelApi::Responses
            || self.provider.authentication != Authentication::ChatGptOAuth
        {
            return Ok(());
        }
        let prepared = self
            .prepare(&LlmRequest::new(&self.config.model, vec![]), true)
            .await?;
        if let Some(client) = prepared.entry.responses.clone() {
            prepared.entry.preconnect(&self.context.stop, async move {
                let _ = REQUEST.scope(prepared.scope, client.preconnect()).await;
            });
        }
        Ok(())
    }
}

fn captured(
    provider: &Provider,
    model: &str,
    authorized: RequestAdapter,
    encrypted: bool,
) -> RequestAdapter {
    let adapter = if let Some(policy) = super::hosted::adapter(provider, model) {
        Arc::new(move |body: &mut Value, headers: &mut HeaderMap| {
            policy(body, headers)?;
            authorized(body, headers)
        }) as RequestAdapter
    } else {
        authorized
    };
    if provider.api == ModelApi::Responses {
        super::hosted::responses(adapter, encrypted)
    } else {
        adapter
    }
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        &self.config.model
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.metadata.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.metadata.uses_interactions_api()
    }
    fn max_cost_micro_usd(&self, request: &LlmRequest) -> Option<u64> {
        self.metadata.max_cost_micro_usd(request)
    }
    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> Result<LlmResponseStream, AdkError> {
        let prepared = self.prepare(&request, stream).await?;
        super::history::generate(
            request,
            prepared.origin,
            super::history::Window::new(&self.provider, &self.config.model),
            move |request| async move {
                let model = super::errors::wrap(prepared.entry, prepared.secrets);
                let responses = scoped(prepared.scope, model, request, prepared.stream).await?;
                Ok(match prepared.tools {
                    Some(tools) => claude::restore(responses, tools),
                    None => responses,
                })
            },
        )
        .await
    }
}

async fn current(
    ingress: &Arc<Ingress>,
    provider: &Provider,
    config: &SessionConfig,
    stop: &CancellationToken,
) -> Result<(Provider, crate::store::login::Authorization), Fault> {
    let reference = config
        .credential
        .clone()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "provider sign-in is required"))?;
    let options = crate::providers::login::effective(provider)?;
    let authorized = ingress
        .authorization(
            reference,
            provider.id,
            provider.authentication,
            &options,
            stop,
        )
        .await?;
    let mut provider = provider.clone();
    provider.endpoint = authorized.grant.endpoint().to_owned();
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = ingress.authorization_endpoint.lock().unwrap().as_ref() {
        provider.endpoint = endpoint.clone();
    }
    Ok((provider, authorized))
}

fn pooled(
    registry: &registry::Registry,
    provider: &Provider,
    config: &SessionConfig,
    session: sailry_protocol::SessionId,
    reasoning: reasoning::Resolved,
    purpose: Purpose,
    authorization: &crate::store::login::Authorization,
) -> Result<Arc<registry::Entry>, Fault> {
    let adapted = matches!(
        provider.api,
        ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::Anthropic
    );
    let websocket = matches!(purpose, Purpose::Main)
        .then(|| {
            registry
                .capabilities
                .options(provider, &config.model, session)
        })
        .flatten();
    let fingerprint = registry::fingerprint(
        provider,
        config,
        reasoning.effort,
        (!adapted).then_some(authorization.credential.revision),
        websocket.is_some(),
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
            reasoning,
            Some(Arc::new(adapter)),
            Some(Arc::new(observe)),
            websocket,
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
