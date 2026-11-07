use super::*;
use crate::providers::request::{self as policy, Metadata, Placement, Prepared};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use sailry_protocol::{RequestId, SessionId};

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
impl types::Model for Model {
    async fn call(
        &self,
        mut request: Request,
        cancel: CancellationToken,
    ) -> Result<ModelStream, Error> {
        let resolved = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::new(ErrorKind::Cancelled, "model request was cancelled")),
            result = policy::current(&self.ingress, &self.provider, self.config.credential.as_ref(), &self.stop) => result.map_err(errors::provider)?,
        };
        let claude = resolved.provider.authentication == Authentication::ClaudeOAuth;
        let tool_names = request_names(&request.tools, &request.messages, claude);
        let user = request
            .messages
            .iter()
            .rev()
            .find(|message| !message.metadata.turn_context)
            .is_some_and(|message| {
                message.role == Role::User
                    && !message
                        .content
                        .iter()
                        .any(|block| matches!(block, Content::ToolResult(_)))
            });
        let prepared = policy::prepare(
            &resolved.provider,
            &resolved.credentials,
            Metadata {
                model: &request.model,
                session: self.session,
                request: RequestId::new(),
                system: &request.system,
                tool_names: &tool_names,
                delegated: self.delegated,
                user,
                vision: request
                    .messages
                    .iter()
                    .flat_map(|message| &message.content)
                    .any(|block| matches!(block, Content::Image { .. })),
            },
        )
        .map_err(errors::provider)?;
        if claude {
            let first = request
                .messages
                .iter()
                .filter(|message| {
                    !message.metadata.turn_context
                        && message.role == Role::User
                        && !message
                            .content
                            .iter()
                            .any(|block| matches!(block, Content::ToolResult(_)))
                })
                .find_map(|message| {
                    message
                        .content
                        .iter()
                        .find_map(|block| match block {
                            Content::Text { text } if !text.trim().is_empty() => Some(text.clone()),
                            _ => None,
                        })
                        .or_else(|| {
                            message
                                .content
                                .iter()
                                .any(|block| matches!(block, Content::Image { .. }))
                                .then(String::new)
                        })
                })
                .unwrap_or_default();
            request
                .params
                .insert("sailry_claude_first_user".into(), first.into());
        }
        apply(&prepared, &mut request);
        let provider = factory::native(
            &self.ingress.model_clients,
            &resolved.provider,
            &self.config.model,
            self.session,
            "",
            prepared.cached_headers.clone(),
        )
        .map_err(errors::provider)?;
        let provider = errors::wrap(provider, prepared.secrets);
        let mut repaired = false;
        let (first, responses) = loop {
            let sent_version = crate::providers::claude::version::current();
            if claude {
                request
                    .params
                    .insert("sailry_claude_version".into(), sent_version.clone().into());
            }
            let started = provider.call(request.clone(), cancel.clone()).await;
            let mut responses = match started {
                Err(error)
                    if claude
                        && !repaired
                        && !cancel.is_cancelled()
                        && crate::providers::claude::version::adopt(
                            &error.to_string(),
                            &sent_version,
                        ) =>
                {
                    repaired = true;
                    continue;
                }
                result => result?,
            };
            let first = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Error::new(ErrorKind::Cancelled, "model request was cancelled")),
                first = responses.next() => first,
            };
            if let Some(Err(error)) = &first
                && claude
                && !repaired
                && !cancel.is_cancelled()
                && crate::providers::claude::version::adopt(&error.to_string(), &sent_version)
            {
                repaired = true;
                continue;
            }
            break (first, responses);
        };
        let names = prepared.names.reverse;
        Ok(Box::pin(stream::iter(first).chain(responses).map(
            move |event| {
                event.map(|mut event| {
                    match &mut event {
                        Event::ToolCall { call, .. } => {
                            if let Some(original) = names.get(&call.name) {
                                call.name.clone_from(original);
                            }
                        }
                        Event::ToolCallDelta {
                            name: Some(name), ..
                        } => {
                            if let Some(original) = names.get(name) {
                                name.clone_from(original);
                            }
                        }
                        _ => {}
                    }
                    event
                })
            },
        )))
    }
}

fn request_names<'a>(tools: &'a [Tool], messages: &'a [Message], claude: bool) -> Vec<&'a str> {
    let mut names: Vec<_> = tools.iter().map(|tool| tool.name.as_str()).collect();
    if claude {
        for block in messages.iter().flat_map(|message| &message.content) {
            if let Content::ToolCall(call) = block
                && call.error.is_none()
                && !names.contains(&call.name.as_str())
            {
                names.push(&call.name);
            }
        }
    }
    names
}

fn apply(prepared: &Prepared, request: &mut Request) {
    for name in prepared.remove_params {
        match *name {
            "max_tokens" => request.max_tokens = None,
            "temperature" => request.temperature = None,
            _ => {}
        }
        request.params.remove(*name);
    }
    request.params.extend(prepared.parameters.clone());
    match prepared.system.placement {
        Placement::Instructions => request.system.clear(),
        Placement::Native => request.system.clone_from(&prepared.system.text),
    }
    for tool in &mut request.tools {
        if let Some(wire) = prepared.names.forward.get(&tool.name) {
            tool.name.clone_from(wire);
        }
    }
    for block in request
        .messages
        .iter_mut()
        .flat_map(|message| &mut message.content)
    {
        match block {
            Content::ToolCall(call) if call.error.is_none() => {
                if let Some(wire) = prepared.names.forward.get(&call.name) {
                    call.name.clone_from(wire);
                }
            }
            Content::ToolResult(result) => {
                if let Some(wire) = prepared.names.forward.get(&result.name) {
                    result.name.clone_from(wire);
                }
            }
            _ => {}
        }
    }
    request.headers.extend(prepared.headers.clone());
}
