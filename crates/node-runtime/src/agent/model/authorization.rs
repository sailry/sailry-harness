//! Each request resolves the turn's captured authorization through the Node owner.
use super::*;
use crate::providers::request::{self as policy, Metadata, Placement, Prepared};
use async_trait::async_trait;
use futures::StreamExt;
use goose_provider_types::{
    base::{MessageStream, Provider as GdkProvider},
    conversation::message::{Message, MessageContentBlock},
    errors::ProviderError,
    model::ModelConfig,
};
use sailry_protocol::{RequestId, SessionId};

pub(super) use policy::canonical;

#[cfg(test)]
mod tests;

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
            let resolved = policy::current(
                &self.ingress,
                &self.provider,
                self.config.credential.as_ref(),
                &self.stop,
            )
            .await?;
            let headers = policy::headers(&resolved.provider, &resolved.credentials)?;
            let fingerprint =
                registry::fingerprint(&resolved.provider, &self.config, resolved.revision)?;
            self.ingress
                .model_clients
                .get(self.session, fingerprint, || {
                    factory::native(
                        &self.ingress.model_clients,
                        &resolved.provider,
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
        let resolved = policy::current(
            &self.ingress,
            &self.provider,
            self.config.credential.as_ref(),
            &self.stop,
        )
        .await
        .map_err(errors::provider)?;
        let tool_names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        let user = messages
            .iter()
            .rev()
            .find(|message| !message.is_turn_context())
            .is_some_and(|message| {
                message.role == rmcp::model::Role::User && !message.is_tool_response()
            });
        let prepared = policy::prepare(
            &resolved.provider,
            &resolved.credentials,
            Metadata {
                model: &config.model_name,
                session: self.session,
                request: RequestId::new(),
                system,
                tool_names: &tool_names,
                delegated: self.delegated,
                user,
                vision: messages
                    .iter()
                    .flat_map(|message| &message.content)
                    .any(|block| matches!(block, MessageContentBlock::Image(_))),
            },
        )
        .map_err(errors::provider)?;
        let fingerprint =
            registry::fingerprint(&resolved.provider, &self.config, resolved.revision)
                .map_err(errors::provider)?;
        let mut config = config.clone();
        let mut system = system.to_owned();
        let mut messages = messages.to_vec();
        let mut tools = tools.to_vec();
        apply(
            &prepared,
            &mut config,
            &mut system,
            &mut messages,
            &mut tools,
        )?;
        let provider = self
            .ingress
            .model_clients
            .get(self.session, fingerprint, || {
                factory::native(
                    &self.ingress.model_clients,
                    &resolved.provider,
                    &self.config.model,
                    self.session,
                    "",
                    prepared.cached_headers,
                )
            })
            .map_err(errors::provider)?;
        let provider = errors::wrap(provider, prepared.secrets);
        let responses = provider.stream(&config, &system, &messages, &tools).await?;
        let names = prepared.names.reverse;
        Ok(Box::pin(responses.map(move |item| {
            item.map(|(mut message, usage)| {
                if let Some(message) = &mut message {
                    for block in &mut message.content {
                        if let MessageContentBlock::ToolRequest(request) = block
                            && let Ok(call) = &mut request.tool_call
                            && let Some(original) = names.get(call.name.as_ref())
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

fn apply(
    prepared: &Prepared,
    config: &mut ModelConfig,
    system: &mut String,
    messages: &mut [Message],
    tools: &mut [rmcp::model::Tool],
) -> Result<(), ProviderError> {
    for name in prepared.remove_params {
        match *name {
            "max_tokens" => config.max_tokens = None,
            "temperature" => config.temperature = None,
            _ => {}
        }
    }
    if !prepared.parameters.is_empty() {
        *config = config
            .clone()
            .with_merged_request_params(prepared.parameters.clone().into_iter().collect());
    }
    match prepared.system.placement {
        Placement::Instructions => system.clear(),
        Placement::Native => system.clone_from(&prepared.system.text),
    }
    for tool in tools {
        if let Some(wire) = prepared.names.forward.get(tool.name.as_ref()) {
            tool.name = wire.clone().into();
        }
    }
    for message in messages {
        for block in &mut message.content {
            if let MessageContentBlock::ToolRequest(request) = block
                && let Ok(call) = &mut request.tool_call
                && let Some(wire) = prepared.names.forward.get(call.name.as_ref())
            {
                call.name = wire.clone().into();
            }
        }
    }
    let headers = config.request_headers.get_or_insert_with(Default::default);
    for (name, value) in &prepared.headers {
        headers.insert(
            name.as_str().into(),
            value
                .to_str()
                .map_err(|_| {
                    ProviderError::InvalidValue("model authorization header is invalid".into())
                })?
                .into(),
        );
    }
    Ok(())
}
