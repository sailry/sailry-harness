use super::super::types::{Error, ErrorKind, Event, Finish as Reason, ToolCall, ToolError, Usage};
use sailry_protocol::{
    Fault, Secret,
    plugin::model_adapter::wire::{Class, Delta, Finish},
};
use std::collections::BTreeMap;

#[derive(Default)]
struct Call {
    index: usize,
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(super) struct Accumulator {
    calls: BTreeMap<u64, Call>,
    usage: Usage,
    finish: Option<Finish>,
    index: usize,
    content: Option<bool>,
    signature: String,
}

impl Accumulator {
    pub(super) fn finished(&self) -> bool {
        self.finish.is_some()
    }

    fn part(&mut self, thinking: bool) -> usize {
        if self.content != Some(thinking) {
            self.index += 1;
            self.content = Some(thinking);
        }
        self.index
    }

    pub(super) fn push(
        &mut self,
        deltas: Vec<Delta>,
        secrets: &[Secret],
    ) -> Result<Vec<Event>, Error> {
        let mut output = Vec::new();
        for delta in deltas {
            match delta {
                Delta::Reasoning { text, replay } => {
                    let index = self.part(true);
                    if let Some(signature) = replay {
                        self.signature.push_str(&signature);
                    }
                    if let Some(text) = text {
                        output.push(Event::ReasoningDelta {
                            index,
                            text: Some(text),
                            signature: None,
                        });
                    }
                }
                Delta::Text { text } => {
                    self.flush(&mut output);
                    if !text.is_empty() {
                        output.push(Event::TextDelta {
                            index: self.part(false),
                            text,
                        });
                    }
                }
                Delta::ToolCall {
                    index,
                    id,
                    name,
                    arguments_delta,
                } => {
                    self.flush(&mut output);
                    let call = self.calls.entry(index).or_insert_with(|| {
                        self.index += 1;
                        self.content = None;
                        Call {
                            index: self.index,
                            ..Call::default()
                        }
                    });
                    if let Some(id) = &id {
                        call.id.clone_from(id);
                    }
                    if let Some(name) = &name {
                        call.name.push_str(name);
                    }
                    if let Some(arguments) = &arguments_delta {
                        call.arguments.push_str(arguments);
                    }
                    output.push(Event::ToolCallDelta {
                        index: call.index,
                        id,
                        name,
                        arguments: arguments_delta.unwrap_or_default(),
                    });
                }
                Delta::Usage {
                    input_tokens,
                    output_tokens,
                    cached_input_tokens,
                    reasoning_tokens,
                } => {
                    if input_tokens.is_some() {
                        self.usage.input = input_tokens;
                    }
                    if output_tokens.is_some() {
                        self.usage.output = output_tokens;
                    }
                    if cached_input_tokens.is_some() {
                        self.usage.cache_read = cached_input_tokens;
                    }
                    if reasoning_tokens.is_some() {
                        self.usage.reasoning = reasoning_tokens;
                    }
                }
                Delta::Finish { reason } => {
                    self.flush(&mut output);
                    self.finish = Some(reason);
                }
                Delta::Error {
                    class,
                    message,
                    retry_after_ms,
                    ..
                } => return Err(classified(class, &message, retry_after_ms, secrets)),
            }
        }
        Ok(output)
    }

    fn flush(&mut self, output: &mut Vec<Event>) {
        if !self.signature.is_empty() {
            output.push(Event::ReasoningDelta {
                index: self.index,
                text: None,
                signature: Some(std::mem::take(&mut self.signature)),
            });
        }
    }

    pub(super) fn finish(mut self, model: &str) -> Result<Vec<Event>, Error> {
        let reason = self.finish.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidResponse,
                "model stream ended before completion",
            )
        })?;
        let mut output = Vec::new();
        for (_, call) in self.calls {
            let parsed =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&call.arguments);
            let (arguments, error) = match parsed {
                Ok(arguments) => (serde_json::Value::Object(arguments), None),
                Err(_) => (
                    serde_json::Value::Null,
                    Some(ToolError {
                        code: -32602,
                        message: "tool arguments must be a JSON object".into(),
                        data: None,
                    }),
                ),
            };
            output.push(Event::ToolCall {
                index: call.index,
                call: ToolCall {
                    id: call.id,
                    name: call.name,
                    arguments,
                    error,
                    metadata: None,
                    tool_meta: None,
                },
            });
        }
        self.usage.model = Some(model.into());
        self.usage.total = self
            .usage
            .input
            .zip(self.usage.output)
            .map(|(input, output)| input.saturating_add(output));
        output.push(Event::Usage(self.usage));
        output.push(Event::Finish(match reason {
            Finish::Stop => Reason::Stop,
            Finish::ToolCalls => Reason::ToolCalls,
            Finish::Length => Reason::Length,
            Finish::ContentFilter => Reason::ContentFilter,
            Finish::Other => Reason::Other("other".into()),
        }));
        Ok(output)
    }
}

pub(super) fn output(delta: &Delta) -> bool {
    match delta {
        Delta::Text { text } => !text.is_empty(),
        Delta::Reasoning { text, replay } => {
            text.as_deref().is_some_and(|text| !text.is_empty())
                || replay.as_deref().is_some_and(|replay| !replay.is_empty())
        }
        Delta::ToolCall { .. } => true,
        Delta::Usage { .. } | Delta::Finish { .. } | Delta::Error { .. } => false,
    }
}

pub(super) fn http_class(status: u16) -> Class {
    match status {
        401 => Class::Authentication,
        403 => Class::Permission,
        429 => Class::RateLimited,
        408 | 500..=599 => Class::Unavailable,
        400..=499 => Class::InvalidRequest,
        _ => Class::Unknown,
    }
}

pub(super) fn classified(
    class: Class,
    message: &str,
    retry_after_ms: Option<u64>,
    secrets: &[Secret],
) -> Error {
    let kind = match class {
        Class::Authentication | Class::Permission => ErrorKind::Authentication,
        Class::RateLimited => ErrorKind::RateLimited,
        Class::Quota => ErrorKind::Quota,
        Class::ContextOverflow => ErrorKind::ContextOverflow,
        Class::InvalidRequest => ErrorKind::InvalidRequest,
        Class::Unsupported => ErrorKind::Unsupported,
        _ => ErrorKind::Unavailable,
    };
    Error {
        kind,
        message: super::super::errors::safe(message, secrets),
        retry_after: retry_after_ms.map(std::time::Duration::from_millis),
    }
}

pub(super) fn fault(error: Fault) -> Error {
    super::super::errors::provider(error)
}
pub(super) fn resource() -> Error {
    Error::new(ErrorKind::Other, "model adapter resource budget exceeded")
}
pub(super) fn translation(error: Fault) -> Error {
    fault(error)
}
pub(super) fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidRequest, message)
}
