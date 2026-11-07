use super::{registry, types};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use rig_core::streaming::{CompletionStream, Item, StreamEvent};
use sailry_protocol::{
    Fault, Secret, SessionId,
    conversation::{ModelApi, Provider},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use types::{Content, Error, ErrorKind, Event, ModelStream, Request, Usage};

mod anthropic;
mod cloud;
mod codec;
mod errors;
mod factory;
mod schema;
mod transport;

#[cfg(test)]
mod tests;

pub(super) struct RigModel {
    client: reqwest::Client,
    provider: Provider,
    model: String,
    session: SessionId,
    key: Secret,
    headers: HeaderMap,
    secrets: Arc<Vec<Secret>>,
    bound: tokio::sync::OnceCell<factory::Binding>,
}

impl RigModel {
    pub(super) fn new(
        registry: &registry::Registry,
        provider: &Provider,
        model: &str,
        session: SessionId,
        key: &str,
        headers: HeaderMap,
    ) -> Result<Self, Fault> {
        let endpoint = crate::providers::endpoint_url(&provider.endpoint)?;
        let mut secrets = vec![Secret::new(key.into())];
        for (name, value) in &headers {
            if (value.is_sensitive()
                || matches!(
                    name.as_str(),
                    "authorization" | "x-api-key" | "x-goog-api-key" | "api-key"
                ))
                && let Ok(value) = value.to_str()
            {
                secrets.push(Secret::new(value.into()));
                if let Some(token) = value.strip_prefix("Bearer ") {
                    secrets.push(Secret::new(token.into()));
                }
            }
        }
        Ok(Self {
            client: registry.http(Some(endpoint.origin()))?,
            provider: provider.clone(),
            model: model.into(),
            session,
            key: Secret::new(key.into()),
            headers,
            secrets: Arc::new(secrets),
            bound: tokio::sync::OnceCell::new(),
        })
    }
}

#[async_trait]
impl types::Model for RigModel {
    async fn call(
        &self,
        mut request: Request,
        cancel: CancellationToken,
    ) -> Result<ModelStream, Error> {
        if !request.model.is_empty() && request.model != self.model {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                "the request model differs from the captured model",
            ));
        }
        request.model = self.model.clone();
        let binding = self.bound.get_or_try_init(|| {
            factory::bind(
                self.client.clone(),
                &self.provider,
                &self.model,
                self.session,
                self.key.expose(),
                self.headers.clone(),
            )
        });
        let model = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            result = binding => result?,
        };
        request.model = model.underlying.clone();
        let request = codec::request(request, model.api)?;
        let stream = model
            .model
            .stream(request)
            .map_err(|error| errors::provider(error, &self.secrets))?;
        let reading = Reading {
            api: model.api,
            stream: Some(stream),
            pending: VecDeque::new(),
            calls: BTreeMap::new(),
            usage: Usage::default(),
            cancel,
            secrets: self.secrets.clone(),
        };
        Ok(Box::pin(futures::stream::unfold(
            reading,
            |mut reading| async {
                loop {
                    if let Some(event) = reading.pending.pop_front() {
                        return Some((event, reading));
                    }
                    let stream = reading.stream.as_mut()?;
                    let next = tokio::select! {
                        biased;
                        _ = reading.cancel.cancelled() => {
                            let usage = codec::usage(&stream.partial());
                            reading.stream = None;
                            reading.usage(usage);
                            reading.pending.push_back(Err(cancelled()));
                            continue;
                        }
                        next = stream.next() => next,
                    };
                    match next {
                        Some(Ok(Item::Event(event))) => {
                            if let Err(error) = reading.event(event) {
                                reading.stream = None;
                                reading.pending.push_back(Err(error));
                            }
                        }
                        Some(Ok(Item::Unknown(_))) => {}
                        Some(Err(error)) => {
                            if let Some(stream) = &reading.stream {
                                let usage = codec::usage(&stream.partial());
                                reading.usage(usage);
                            }
                            reading.stream = None;
                            reading
                                .pending
                                .push_back(Err(errors::provider(error, &reading.secrets)));
                        }
                        None => {
                            let stream = reading.stream.take()?;
                            let response = tokio::select! {
                                biased;
                                _ = reading.cancel.cancelled() => Err(cancelled()),
                                result = stream.finish() => result.map_err(|error| errors::provider(error, &reading.secrets)),
                            };
                            match response {
                                Ok(response) => {
                                    reading.usage(codec::usage(&response));
                                    let unfinished: std::collections::BTreeSet<_> = response
                                        .unfinished_calls()
                                        .map(|call| call.id.to_string())
                                        .collect();
                                    let finish = codec::finish(response.finish_reason());
                                    let incomplete = reading.api == ModelApi::Anthropic
                                        && !unfinished.is_empty()
                                        && matches!(
                                            finish,
                                            types::Finish::Stop
                                                | types::Finish::ToolCalls
                                                | types::Finish::Length
                                        );
                                    if let Some(message) = &response.error
                                        && !incomplete
                                    {
                                        reading.pending.push_back(Err(Error::new(
                                            ErrorKind::InvalidResponse,
                                            super::errors::safe(message, &reading.secrets),
                                        )));
                                        continue;
                                    }
                                    for (index, mut call) in std::mem::take(&mut reading.calls) {
                                        if unfinished.contains(&call.id) {
                                            if reading.api != ModelApi::Anthropic {
                                                continue;
                                            }
                                            call.error = Some(types::ToolError {
                                                code: -32602,
                                                message: "tool call ended before its content block was complete".into(),
                                                data: None,
                                            });
                                        }
                                        reading
                                            .pending
                                            .push_back(Ok(Event::ToolCall { index, call }));
                                    }
                                    reading.pending.push_back(Ok(Event::Finish(finish)));
                                }
                                Err(error) => reading.pending.push_back(Err(error)),
                            }
                        }
                    }
                }
            },
        )))
    }
}

struct Reading {
    api: ModelApi,
    stream: Option<CompletionStream>,
    pending: VecDeque<Result<Event, Error>>,
    calls: BTreeMap<usize, types::ToolCall>,
    usage: Usage,
    cancel: CancellationToken,
    secrets: Arc<Vec<Secret>>,
}

impl Reading {
    fn event(&mut self, event: StreamEvent) -> Result<(), Error> {
        let index = event.part().index();
        let event = match event {
            StreamEvent::Start {
                name: Some(name), ..
            } => Some(Event::ToolCallDelta {
                index,
                id: None,
                name: Some(name.as_str().into()),
                arguments: String::new(),
            }),
            StreamEvent::Text { text, .. } => Some(Event::TextDelta { index, text }),
            StreamEvent::Reasoning { text, .. } => Some(Event::ReasoningDelta {
                index,
                text: Some(text),
                signature: None,
            }),
            StreamEvent::Arguments { json, .. } => Some(Event::ToolCallDelta {
                index,
                id: None,
                name: None,
                arguments: json,
            }),
            StreamEvent::End {
                content: rig_core::message::AssistantContent::Opaque(_),
                ..
            } => None,
            StreamEvent::End { content, .. } => match codec::content(content)? {
                Content::ToolCall(call) => {
                    self.calls.insert(index, call);
                    None
                }
                Content::Thinking {
                    signature: Some(signature),
                    ..
                } => Some(Event::ReasoningDelta {
                    index,
                    text: None,
                    signature: Some(signature),
                }),
                Content::RedactedThinking { data } => Some(Event::RedactedThinking { index, data }),
                _ => None,
            },
            _ => None,
        };
        if let Some(event) = event {
            self.pending.push_back(Ok(event));
        }
        Ok(())
    }

    fn usage(&mut self, usage: Usage) {
        if usage != self.usage && usage != Usage::default() {
            self.usage = usage.clone();
            self.pending.push_back(Ok(Event::Usage(usage)));
        }
    }
}

fn cancelled() -> Error {
    Error::new(ErrorKind::Cancelled, "model request was cancelled")
}
