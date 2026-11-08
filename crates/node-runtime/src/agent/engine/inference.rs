//! Inference recovery adapted from Goose 14741e9a inference.rs (Apache-2.0).
//! See third_party_licenses/goose-Apache-2.0.txt.
use super::{
    Effect, Session,
    operation::*,
    prompt::{InferenceInput, Preparer},
};
use crate::agent::model::{self, Content, Event, Finish, Message, Usage, types::BlockKind};
use futures::StreamExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

pub(super) mod projection;
#[cfg(test)]
mod tests;

const EMPTY_RESPONSE: &str =
    "The model returned an empty response. Please resend your message to continue.";
const MAX_EMPTY_RETRIES: usize = 3;

#[derive(Default)]
struct Output {
    additional: Vec<Message>,
    usage: Vec<Usage>,
    attempt: usize,
    message: Option<Message>,
    blocks: BTreeMap<(usize, BlockKind), Content>,
    calls: BTreeSet<String>,
    error: Option<Message>,
    logs: Vec<String>,
    finished: bool,
}

impl Output {
    fn begin(&mut self, owner: &str, config: &model::Config) {
        self.attempt = self.usage.len();
        self.blocks.clear();
        self.calls.clear();
        self.finished = false;
        let mut message = Message::assistant().with_generated_id();
        message.metadata.inference = Some(model::types::Inference {
            provider: owner.to_owned(),
            requested_model: config.model_name.clone(),
            ..Default::default()
        });
        message.metadata.operation_logs.clone_from(&self.logs);
        self.message = Some(message);
    }

    fn receive(&mut self, event: Event, emit: &Emitter) {
        let message = self
            .message
            .as_mut()
            .expect("inference attempt was started");
        let mut partial = message.clone();
        partial.content.clear();
        match event {
            Event::TextDelta { index, text } => {
                let block = self
                    .blocks
                    .entry((index, BlockKind::Text))
                    .or_insert_with(|| Content::Text {
                        text: String::new(),
                    });
                if let Content::Text { text: current } = block {
                    current.push_str(&text);
                }
                partial.content.push(Content::Text { text });
            }
            Event::ReasoningDelta {
                index,
                text,
                signature,
            } => {
                let block = self
                    .blocks
                    .entry((index, BlockKind::Thinking))
                    .or_insert_with(|| Content::Thinking {
                        text: String::new(),
                        signature: None,
                    });
                if let Content::Thinking {
                    text: current,
                    signature: replay,
                } = block
                {
                    if let Some(text) = &text {
                        current.push_str(text);
                    }
                    if let Some(signature) = &signature {
                        replay.get_or_insert_with(String::new).push_str(signature);
                    }
                }
                if let Some(text) = text {
                    partial.content.push(Content::Thinking {
                        text,
                        signature: None,
                    });
                }
            }
            Event::RedactedThinking { index, data } => {
                self.blocks.insert(
                    (index, BlockKind::RedactedThinking),
                    Content::RedactedThinking { data },
                );
            }
            Event::ToolCall { index, call } => {
                if self.calls.insert(call.id.clone()) {
                    self.blocks
                        .insert((index, BlockKind::ToolCall), Content::ToolCall(call));
                }
            }
            Event::ToolCallDelta { .. } => {}
            Event::Usage(usage) => {
                if let Some(inference) = &mut message.metadata.inference {
                    inference.resolved_model = usage.model.clone();
                }
                self.usage.truncate(self.attempt);
                self.usage.push(usage);
            }
            Event::Finish(finish) => {
                self.finished = true;
                message.metadata.output_token_limit_reached = finish == Finish::Length;
            }
        }
        if !partial.content.is_empty() {
            emit.message(partial);
        }
    }

    fn empty(&self) -> bool {
        !self
            .message
            .as_ref()
            .is_some_and(|message| message.metadata.output_token_limit_reached)
            && self.blocks.values().all(|content| match content {
                Content::Text { text } => text.trim().is_empty(),
                Content::Thinking { text, signature } => {
                    text.trim().is_empty() && signature.as_ref().is_none_or(String::is_empty)
                }
                _ => false,
            })
    }

    fn take(mut self, cancelled: bool) -> Vec<Effect> {
        let mut effects: Vec<_> = self.additional.into_iter().map(Effect::from).collect();
        effects.extend(self.usage.into_iter().map(Effect::Usage));
        if let Some(mut message) = self.message.take()
            && (!self.blocks.is_empty()
                || !message.content.is_empty()
                || message.metadata.output_token_limit_reached
                || (!cancelled && self.finished))
        {
            message.content.extend(self.blocks.into_values());
            effects.push(message.into());
        }
        effects.extend(self.error.map(Effect::from));
        effects
    }
}

pub(super) struct Inference {
    pub provider: Arc<dyn model::Model>,
    pub config: model::Config,
    pub owner: String,
    pub preparer: Arc<Preparer>,
    output: Mutex<Output>,
}

impl Inference {
    pub(super) fn new(binding: model::Binding, preparer: Arc<Preparer>) -> Self {
        Self {
            provider: binding.provider,
            config: binding.config,
            owner: binding.owner,
            preparer,
            output: Mutex::default(),
        }
    }

    pub(super) fn finalize_cancellation(&self) -> Vec<Effect> {
        std::mem::take(&mut *self.output.lock().unwrap()).take(true)
    }

    pub(super) async fn run(
        &self,
        session: &Session,
        input: impl Future<Output = anyhow::Result<InferenceInput>>,
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        if super::conversation::trailing_error(&session.conversation).is_some()
            || !projection::applies(&session.conversation)
        {
            return not_applicable();
        }
        let conversation = self.preparer.prepare_messages(session).await?;
        if !projection::applies(&conversation) {
            return not_applicable();
        }
        let input = input.await?;
        let prepared = self.preparer.prepare(session, &conversation, input).await?;
        let mut messages = projection::visible(&conversation)?;
        messages.extend(prepared.additional_messages.iter().cloned());
        self.output.lock().unwrap().additional = prepared.additional_messages;
        self.output.lock().unwrap().logs =
            operation_logs(super::conversation::messages_since_kickoff(&conversation)?);
        let messages = projection::prepare(messages, &self.owner);
        let successful_tool = successful_tool_response(&conversation);
        let mut config = self.config.clone();
        if let Some(effort) = session.thinking {
            config.thinking_effort = effort;
        }
        let request = config.request(
            prepared.system_prompt,
            messages,
            prepared.tools.into_iter().map(Into::into).collect(),
        );
        complete(
            self.provider.as_ref(),
            &self.owner,
            &config,
            request,
            successful_tool,
            emit,
            &self.output,
        )
        .await
    }
}

async fn complete(
    provider: &dyn model::Model,
    owner: &str,
    config: &model::Config,
    request: model::Request,
    successful_tool: bool,
    emit: &Emitter,
    pending: &Mutex<Output>,
) -> anyhow::Result<Outcome> {
    let finish = || applied(std::mem::take(&mut *pending.lock().unwrap()).take(false));
    let cancelled = || {
        Ok(Outcome::Applied(Transition {
            effects: std::mem::take(&mut *pending.lock().unwrap()).take(true),
            status: RunStatus::Cancelled,
        }))
    };
    let failed = |error: model::Error| {
        if error.kind == model::ErrorKind::Cancelled && emit.cancel_token().is_cancelled() {
            return cancelled();
        }
        let mut message = Message::assistant()
            .with_error(error.kind, error.message)
            .with_visibility(true, false);
        let mut output = pending.lock().unwrap();
        if output.blocks.is_empty() {
            message.metadata.operation_logs.clone_from(&output.logs);
        }
        output.error = Some(emit.message(message));
        drop(output);
        finish()
    };
    for attempt in 0..=MAX_EMPTY_RETRIES {
        pending.lock().unwrap().begin(owner, config);
        let stream = provider
            .call(request.clone(), emit.cancel_token().clone())
            .await;
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => return failed(error),
        };
        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => pending.lock().unwrap().receive(event, emit),
                Err(error) => return failed(error),
            }
        }
        let mut output = pending.lock().unwrap();
        if !output.empty() {
            drop(output);
            return finish();
        }
        if emit.cancel_token().is_cancelled() {
            drop(output);
            return cancelled();
        }
        output.blocks.clear();
        if successful_tool {
            output.finished = true;
            output.message.as_mut().unwrap().metadata.user_visible = false;
            drop(output);
            return finish();
        }
        if attempt == MAX_EMPTY_RETRIES {
            let mut marker = Message::assistant()
                .with_text(EMPTY_RESPONSE)
                .with_visibility(false, false);
            marker
                .metadata
                .set_operation_note("inference", "empty_response", true.into());
            marker.metadata.operation_logs.clone_from(&output.logs);
            output.message = Some(marker);
            drop(output);
            return finish();
        }
    }
    unreachable!("empty response retries are bounded")
}

fn operation_logs(messages: &[Message]) -> Vec<String> {
    let mut seen: BTreeSet<_> = messages
        .iter()
        .flat_map(|message| &message.metadata.operation_logs)
        .cloned()
        .collect();
    messages
        .iter()
        .filter_map(|message| message.metadata.operations.as_ref())
        .flat_map(|operations| operations.iter())
        .filter_map(|(operation, notes)| {
            notes
                .get("client_log")
                .and_then(serde_json::Value::as_str)
                .map(|line| format!("ops_{operation}: {line}"))
        })
        .filter(|line| seen.insert(line.clone()))
        .collect()
}

fn successful_tool_response(messages: &[Message]) -> bool {
    let Some(message) = messages.last() else {
        return false;
    };
    let mut results = message
        .content
        .iter()
        .filter_map(|content| match content {
            Content::ToolResult(result) => Some(result),
            _ => None,
        })
        .peekable();
    results.peek().is_some()
        && results.all(|result| {
            result
                .result
                .as_ref()
                .is_ok_and(|result| result.is_error != Some(true))
        })
}
