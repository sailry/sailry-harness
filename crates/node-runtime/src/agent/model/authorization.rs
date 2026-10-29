//! Each request resolves the turn's captured authorization through the Node owner.
use super::*;
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_types::{
    base::{MessageStream, Provider as GdkProvider},
    conversation::message::{Message, MessageContentBlock},
    errors::ProviderError,
    model::ModelConfig,
};
use reqwest::header::HeaderValue;
use sailry_protocol::SessionId;
use serde_json::json;

pub(super) struct Model {
    ingress: Arc<Ingress>,
    provider: Provider,
    config: SessionConfig,
    session: SessionId,
    delegated: bool,
    stop: CancellationToken,
}

impl Model {
    pub(super) fn new(
        ingress: &Arc<Ingress>,
        provider: &Provider,
        config: &SessionConfig,
        session: SessionId,
        delegated: bool,
        stop: &CancellationToken,
    ) -> Self {
        Self {
            ingress: ingress.clone(),
            provider: provider.clone(),
            config: config.clone(),
            session,
            delegated,
            stop: stop.clone(),
        }
    }
}

#[async_trait]
impl GdkProvider for Model {
    fn get_name(&self) -> &str {
        "sailry_authorized"
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        if let Some(limit) = override_limit {
            return limit;
        }
        let resolved = async {
            let (effective, authorized) =
                current(&self.ingress, &self.provider, &self.config, &self.stop).await?;
            let options = crate::providers::login::effective(&self.provider)?;
            let headers = authorized.grant.headers(&options)?;
            let fingerprint = registry::fingerprint(
                &effective,
                &self.config,
                Some(authorized.credential.revision),
            )?;
            self.ingress
                .model_clients
                .get(self.session, fingerprint, || {
                    factory::native(
                        &self.ingress.model_clients,
                        &effective,
                        &self.config.model,
                        self.session,
                        "",
                        headers,
                    )
                })
        }
        .await;
        match resolved {
            Ok(provider) => provider.get_context_limit(model, None).await,
            Err(_) => {
                goose_provider_types::context_limit::ContextLimitResolver::new(self.get_name())
                    .resolve_local(model, None)
            }
        }
    }

    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[rmcp::model::Tool],
    ) -> Result<MessageStream, ProviderError> {
        let (effective, authorized) =
            current(&self.ingress, &self.provider, &self.config, &self.stop)
                .await
                .map_err(errors::provider)?;
        let options =
            crate::providers::login::effective(&self.provider).map_err(errors::provider)?;
        let mut headers = authorized
            .grant
            .headers(&options)
            .map_err(errors::provider)?;
        let secrets = authorized.grant.secrets(&headers);
        let cached_headers = headers.clone();
        let fingerprint = registry::fingerprint(
            &effective,
            &self.config,
            Some(authorized.credential.revision),
        )
        .map_err(errors::provider)?;
        let mut config = config.clone();
        let mut system = system.to_owned();
        let mut messages = messages.to_vec();
        let mut tools = tools.to_vec();
        let names: Vec<_> = tools.iter().map(|tool| tool.name.to_string()).collect();
        let claude = matches!(
            authorized.grant,
            crate::providers::login::Grant::ClaudeOAuth { .. }
        );
        match &authorized.grant {
            crate::providers::login::Grant::ChatGptOAuth { .. } => {
                // The Codex endpoint requires instructions rather than a system input item.
                config.max_tokens = None;
                config.temperature = None;
                config = config.with_merged_request_params(
                    [
                        ("instructions".into(), json!(system)),
                        ("prompt_cache_key".into(), json!(self.session.to_string())),
                    ]
                    .into(),
                );
                system.clear();
                headers.insert("session-id", header(&self.session.to_string())?);
                headers.insert(
                    "x-codex-routing-hint",
                    header(&format!("model={}", config.model_name))?,
                );
            }
            crate::providers::login::Grant::ChatGpt { .. } => {
                config.max_tokens = None;
                config.temperature = None;
            }
            crate::providers::login::Grant::ClaudeOAuth { .. } => {
                // Pi's reviewed subscription identity is an authorization requirement.
                // Source: pi 4270522473e677ddba7b093caa0064a3df51d459 (MIT).
                system = format!(
                    "You are Claude Code, Anthropic's official CLI for Claude.\n\n{system}"
                );
                for tool in &mut tools {
                    tool.name = canonical(&tool.name).to_string().into();
                }
                for message in &mut messages {
                    for block in &mut message.content {
                        if let MessageContentBlock::ToolRequest(request) = block
                            && let Ok(call) = &mut request.tool_call
                        {
                            call.name = canonical(&call.name).to_string().into();
                        }
                    }
                }
            }
            crate::providers::login::Grant::Copilot { .. } => {
                let user = !self.delegated
                    && messages.last().is_some_and(|message| {
                        message.role == rmcp::model::Role::User && !message.is_tool_response()
                    });
                headers.insert(
                    "openai-intent",
                    HeaderValue::from_static("conversation-agent"),
                );
                headers.insert(
                    "x-initiator",
                    HeaderValue::from_static(if user { "user" } else { "agent" }),
                );
                headers.insert(
                    "x-request-id",
                    header(&sailry_protocol::RequestId::new().to_string())?,
                );
                if messages
                    .iter()
                    .flat_map(|message| &message.content)
                    .any(|block| matches!(block, MessageContentBlock::Image(_)))
                {
                    headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
                }
            }
            crate::providers::login::Grant::Plugin { .. } => {}
        }
        let request_headers = config.request_headers.get_or_insert_with(Default::default);
        for (name, value) in &headers {
            request_headers.insert(
                name.as_str().into(),
                value
                    .to_str()
                    .map_err(|_| {
                        ProviderError::InvalidValue("model authorization header is invalid".into())
                    })?
                    .into(),
            );
        }
        let provider = self
            .ingress
            .model_clients
            .get(self.session, fingerprint, || {
                factory::native(
                    &self.ingress.model_clients,
                    &effective,
                    &self.config.model,
                    self.session,
                    "",
                    cached_headers,
                )
            })
            .map_err(errors::provider)?;
        let provider = errors::wrap(provider, secrets);
        let responses = provider.stream(&config, &system, &messages, &tools).await?;
        Ok(Box::pin(responses.map(move |item| {
            item.map(|(mut message, usage)| {
                if claude && let Some(message) = &mut message {
                    for block in &mut message.content {
                        if let MessageContentBlock::ToolRequest(request) = block
                            && let Ok(call) = &mut request.tool_call
                            && let Some(original) = names
                                .iter()
                                .find(|name| name.eq_ignore_ascii_case(&call.name))
                        {
                            call.name = original.clone().into();
                        }
                    }
                }
                (message, usage)
            })
        })))
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
    let mut effective = provider.clone();
    effective.endpoint = authorized.grant.endpoint().into();
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = ingress.authorization_endpoint.lock().unwrap().as_ref() {
        effective.endpoint = endpoint.clone();
    }
    Ok((effective, authorized))
}

fn header(value: &str) -> Result<HeaderValue, ProviderError> {
    HeaderValue::from_str(value)
        .map_err(|_| ProviderError::InvalidValue("model authorization header is invalid".into()))
}

fn canonical(name: &str) -> &str {
    const TOOLS: &[&str] = &[
        "Read",
        "Write",
        "Edit",
        "Bash",
        "Grep",
        "Glob",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
        "KillShell",
        "NotebookEdit",
        "Skill",
        "Task",
        "TaskOutput",
        "TodoWrite",
        "WebFetch",
        "WebSearch",
    ];
    TOOLS
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
        .unwrap_or(name)
}
