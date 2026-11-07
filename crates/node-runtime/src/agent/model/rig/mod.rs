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
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use types::{Content, Error, ErrorKind, Event, ModelStream, Request, Usage};

mod anthropic;
mod cloud;
mod codec;
mod errors;
mod factory;
mod media;
mod parameters;
mod responses;
mod schema;
mod thinking;
mod transport;
mod wire;

pub(super) fn adaptive(model: &str) -> bool {
    rig_core::catalog::Catalog::builtin()
        .resolve(&format!("anthropic/{model}"))
        .is_ok_and(|resolved| resolved.spec.compat.adaptive_thinking)
}

pub(super) async fn context(
    client: &reqwest::Client,
    provider: &Provider,
    model: &str,
    headers: HeaderMap,
) -> Result<usize, Error> {
    let route = cloud::resolve(client, provider, model, "", &headers).await?;
    Ok(super::gdk::context(ModelApi::AzureAi, &route.underlying))
}

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
        let fresh = if request.headers.is_empty() {
            None
        } else {
            let mut headers = std::mem::take(&mut request.headers);
            for name in ["authorization", "x-api-key", "x-goog-api-key", "api-key"] {
                headers.remove(name);
            }
            if let Some(injection) = self
                .provider
                .binding
                .as_ref()
                .and_then(|binding| binding.key_injection.as_ref())
            {
                headers.remove(&injection.header);
            }
            headers.extend(self.headers.clone());
            let binding = factory::bind(
                self.client.clone(),
                &self.provider,
                &self.model,
                self.session,
                self.key.expose(),
                headers,
            );
            Some(tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = binding => result?,
            })
        };
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
        let model = if let Some(binding) = &fresh {
            binding
        } else {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = binding => result?,
            }
        };
        request.model = model.underlying.clone();
        if model.api == ModelApi::Responses {
            request
                .params
                .entry("store")
                .or_insert_with(|| false.into());
            request
                .params
                .entry("prompt_cache_key")
                .or_insert_with(|| self.session.to_string().into());
        }
        parameters::prepare(&mut request, model.api, self.provider.api);
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
            open_calls: BTreeSet::new(),
            usage: Usage::default(),
            response_delta: false,
            response_text: false,
            inline: BTreeMap::new(),
            inline_thinking: BTreeMap::new(),
            structured_reasoning: false,
            cancel,
            secrets: self.secrets.clone(),
        };
        Ok(Box::pin(futures::stream::unfold(
            reading,
            |mut reading| async {
                loop {
                    if let Some(event) = reading.pending.pop_front() {
                        if matches!(
                            event,
                            Ok(Event::TextDelta { .. }
                                | Event::ReasoningDelta { .. }
                                | Event::RedactedThinking { .. }
                                | Event::ToolCallDelta { .. }
                                | Event::ToolCall { .. })
                        ) && let Some(observed) = super::observation::current()
                        {
                            observed.output(std::time::Instant::now());
                        }
                        return Some((event, reading));
                    }
                    let stream = reading.stream.as_mut()?;
                    let next = tokio::select! {
                        biased;
                        _ = reading.cancel.cancelled() => {
                            let usage = codec::usage(&stream.partial());
                            reading.stream = None;
                            reading.finish_text();
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
                        Some(Ok(Item::Unknown(payload))) => {
                            if reading.api == ModelApi::Responses
                                && let Some(delta) = payload
                                    .value()
                                    .get(responses::TEXT_DELTA)
                                    .and_then(serde_json::Value::as_bool)
                            {
                                reading.response_delta = delta;
                                reading.response_text |= delta;
                            }
                        }
                        Some(Err(error)) => {
                            if let Some(stream) = &reading.stream {
                                let usage = codec::usage(&stream.partial());
                                reading.usage(usage);
                            }
                            reading.finish_text();
                            reading.stream = None;
                            if matches!(error, rig_core::error::ProviderError::Truncated)
                                && reading.api != ModelApi::Responses
                                && reading.open_calls.is_empty()
                            {
                                for (index, call) in std::mem::take(&mut reading.calls) {
                                    reading
                                        .pending
                                        .push_back(Ok(Event::ToolCall { index, call }));
                                }
                                reading
                                    .pending
                                    .push_back(Ok(Event::Finish(types::Finish::Stop)));
                            } else {
                                reading
                                    .pending
                                    .push_back(Err(errors::provider(error, &reading.secrets)));
                            }
                        }
                        None => {
                            let stream = reading.stream.take()?;
                            reading.usage(codec::usage(&stream.partial()));
                            let response = tokio::select! {
                                biased;
                                _ = reading.cancel.cancelled() => Err(cancelled()),
                                result = stream.finish() => result.map_err(|error| errors::provider(error, &reading.secrets)),
                            };
                            match response {
                                Ok(response) => {
                                    reading.finish_text();
                                    reading.usage(codec::usage(&response));
                                    if reading.api == ModelApi::Responses && !reading.response_text
                                    {
                                        for (index, content) in response.choice.iter().enumerate() {
                                            if let rig_core::message::AssistantContent::Text(text) =
                                                content
                                                && !text.text.is_empty()
                                            {
                                                reading.pending.push_back(Ok(Event::TextDelta {
                                                    index,
                                                    text: text.text.clone(),
                                                }));
                                            }
                                        }
                                    }
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
                                        && !(reading.api == ModelApi::Responses
                                            && response.raw["status"] == "incomplete")
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
                                Err(error) => {
                                    reading.finish_text();
                                    reading.pending.push_back(Err(error));
                                }
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
    open_calls: BTreeSet<usize>,
    usage: Usage,
    response_delta: bool,
    response_text: bool,
    inline: BTreeMap<usize, thinking::Filter>,
    inline_thinking: BTreeMap<usize, String>,
    structured_reasoning: bool,
    cancel: CancellationToken,
    secrets: Arc<Vec<Secret>>,
}

impl Reading {
    fn event(&mut self, event: StreamEvent) -> Result<(), Error> {
        let index = event.part().index();
        match &event {
            StreamEvent::Start {
                kind: rig_core::streaming::PartKind::ToolCall,
                ..
            }
            | StreamEvent::Arguments { .. } => {
                self.open_calls.insert(index);
            }
            StreamEvent::End {
                content: rig_core::message::AssistantContent::ToolCall(_),
                ..
            } => {
                self.open_calls.remove(&index);
            }
            _ => {}
        }
        let event = match event {
            StreamEvent::Start {
                name: Some(name), ..
            } => Some(Event::ToolCallDelta {
                index,
                id: None,
                name: Some(name.as_str().into()),
                arguments: String::new(),
            }),
            StreamEvent::Text { text, .. }
                if self.api != ModelApi::Responses || self.response_delta =>
            {
                let text = if matches!(self.api, ModelApi::ChatCompletions | ModelApi::DeepSeek) {
                    let output = self.inline.entry(index).or_default().push(&text);
                    self.inline_thinking
                        .entry(index)
                        .or_default()
                        .push_str(&output.thinking);
                    output.text
                } else {
                    text
                };
                (!text.is_empty()).then_some(Event::TextDelta { index, text })
            }
            StreamEvent::Reasoning { text, .. } => {
                self.structured_reasoning |= !text.is_empty();
                Some(Event::ReasoningDelta {
                    index,
                    text: Some(text),
                    signature: None,
                })
            }
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

    fn finish_text(&mut self) {
        for (index, filter) in std::mem::take(&mut self.inline) {
            let output = filter.finish();
            if !output.text.is_empty() {
                self.pending.push_back(Ok(Event::TextDelta {
                    index,
                    text: output.text,
                }));
            }
            self.inline_thinking
                .entry(index)
                .or_default()
                .push_str(&output.thinking);
        }
        for (index, text) in std::mem::take(&mut self.inline_thinking) {
            if !self.structured_reasoning && !text.is_empty() {
                self.pending.push_back(Ok(Event::ReasoningDelta {
                    index,
                    text: Some(text),
                    signature: None,
                }));
            }
        }
    }

    fn usage(&mut self, usage: Usage) {
        let reported = usage.input.is_some()
            || usage.output.is_some()
            || usage.total.is_some()
            || usage.cache_read.is_some()
            || usage.cache_write.is_some()
            || usage.reasoning.is_some()
            || usage.cost.is_some()
            || usage
                .provider
                .as_ref()
                .is_some_and(|value| !value.is_null());
        if reported && usage != self.usage {
            self.usage = usage.clone();
            self.pending.push_back(Ok(Event::Usage(usage)));
        }
    }
}

fn cancelled() -> Error {
    Error::new(ErrorKind::Cancelled, "model request was cancelled")
}
