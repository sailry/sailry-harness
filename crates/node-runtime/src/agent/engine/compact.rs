//! Adapted from Goose ee8500d5, ops_compaction.rs and context_mgmt/mod.rs.
//! Summarization derives from Goose 14741e9a, goose-context-management.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{
    Effect, Session,
    conversation::{EffectiveRole, last_effective_role, messages_since_kickoff, trailing_error},
    operation::{Emitter, Outcome, RunStatus, Transition, applied, not_applicable, yielded_with},
    tokens,
};
use crate::agent::{
    canonical::CompactedFile,
    model::{self, Config, Content, Error, ErrorKind, Event, Finish, Message, Role, Usage},
};
use sailry_link::CancellationToken;
use std::sync::{Arc, Mutex};

mod format;
mod handoff;
mod input;
mod loadout;
mod structured;
mod summarize;
mod templates;
#[cfg(test)]
mod tests;

pub(super) struct Compaction {
    pub provider: Arc<dyn model::Model>,
    pub config: Config,
    pub context: usize,
    pub threshold: f64,
    pub preparer: Arc<super::prompt::Preparer>,
    pub manual: bool,
}
/// Sailry's concise handoff prompt.
const PROMPT: &str = include_str!("compact/prompt.md");
/// Renders the structured fields as Goal, Constraints, State, Decisions,
/// Next Steps and Essential Context.
const RENDER: &str = include_str!("compact/render.md");

pub(super) struct Model {
    pub provider: Arc<dyn model::Model>,
    pub config: Config,
    pub files: Mutex<Vec<CompactedFile>>,
    pub stop: CancellationToken,
}
impl Model {
    /// Only a complete, parseable structured summary is accepted. A response
    /// cut off at the output limit or without the JSON document fails the
    /// compaction instead of replacing history with partial or raw text.
    /// The accepted document is bounded before rendering.
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
    ) -> Result<(Message, Usage), Error> {
        use futures::StreamExt;
        let request = self
            .config
            .request(system.into(), messages.to_vec(), Vec::new());
        let mut stream = self.provider.call(request, self.stop.clone()).await?;
        let mut text = String::new();
        let mut text_block = false;
        let mut received = false;
        let mut limited = false;
        let mut usage = None;
        while let Some(event) = stream.next().await {
            match event? {
                Event::TextDelta { text: delta, .. } => {
                    if !text_block && !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&delta);
                    text_block = true;
                    received = true;
                }
                Event::Usage(reported) => usage = Some(reported),
                Event::Finish(finish) => {
                    limited |= truncated(&finish);
                    received = true;
                }
                Event::ReasoningDelta { .. }
                | Event::RedactedThinking { .. }
                | Event::ToolCall { .. } => {
                    text_block = false;
                    received = true;
                }
                Event::ToolCallDelta { .. } => {}
            }
        }
        if !received && usage.is_none() {
            return Err(Error::new(ErrorKind::Other, "Stream yielded no message"));
        }
        if limited {
            return Err(Error::new(
                ErrorKind::Other,
                "compaction summary reached its output limit",
            ));
        }
        let mut summary = structured::Summary::parse(&text).ok_or_else(|| {
            Error::new(
                ErrorKind::Other,
                "compaction summary was not a structured handoff",
            )
        })?;
        handoff::bound(&mut summary)
            .map_err(|error| Error::new(ErrorKind::Other, error.to_string()))?;
        let document = serde_json::to_string(&summary)
            .map_err(|error| Error::new(ErrorKind::Other, error.to_string()))?;
        let message = Message::assistant().with_text(document);
        *self.files.lock().unwrap() = summary
            .files
            .into_iter()
            .map(|file| CompactedFile {
                path: file.path,
                modified: None,
                summary: Some(match file.key_code {
                    Some(code) => format!("{}\n{code}", file.summary),
                    None => file.summary,
                }),
            })
            .collect();
        Ok((
            message,
            usage.unwrap_or_else(|| Usage {
                model: Some("unknown".into()),
                ..Usage::default()
            }),
        ))
    }
}

/// Provider stop reasons that mean the output limit ended the response.
fn truncated(finish: &Finish) -> bool {
    match finish {
        Finish::Length => true,
        Finish::Other(reason) => matches!(
            reason.to_ascii_lowercase().as_str(),
            "length" | "max_tokens" | "max_output_tokens"
        ),
        _ => false,
    }
}
impl Compaction {
    /// `id` is the summary record identity announced by the in-progress status.
    async fn compact(
        &self,
        session: &Session,
        conversation: &[Message],
        id: &str,
    ) -> anyhow::Result<Vec<Effect>> {
        let model = Model {
            provider: self.provider.clone(),
            config: self.config.clone(),
            files: Mutex::default(),
            stop: self.preparer.stop.clone(),
        };
        let mut projected: Vec<_> = conversation
            .iter()
            .filter(|message| !message.is_turn_context() && message.is_agent_visible())
            .cloned()
            .collect();
        self.preparer.hydrate(&mut projected).await?;
        let input = input::prepare(
            &projected,
            self.context,
            self.config.max_tokens.unwrap_or(8_000) as usize,
        )?;
        let (summary, usage) = summarize::run(&model, &input).await?;
        let summary = summary.with_id(id.to_owned()).with_visibility(false, true);
        let preserved = if self.manual {
            None
        } else {
            // An earlier summary is merged into the new one, never carried beside it.
            conversation.iter().rev().find(|message| {
                message.is_agent_visible()
                    && !message.is_turn_context()
                    && !message
                        .metadata
                        .usage
                        .as_ref()
                        .is_some_and(|usage| usage.is_compaction)
                    && message.role == Role::User
                    && message
                        .content
                        .iter()
                        .any(|content| matches!(content, Content::Text { .. }))
                    && !message.content.iter().any(|content| {
                        matches!(content, Content::ToolCall(_) | Content::ToolResult(_))
                    })
            })
        };
        let is_latest = preserved.is_some_and(|message| {
            conversation.last().and_then(|last| last.id.as_ref()) == message.id.as_ref()
        });
        let continuation = if self.manual {
            "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else if is_latest {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nContinue calling tools as necessary to complete the task."
        };
        let loaded = session.context.loaded_tools.as_deref().unwrap_or_default();
        let mut effects = vec![
            Effect::Compaction {
                summary: Box::new(summary),
                usage,
                files: std::mem::take(&mut *model.files.lock().unwrap()),
                tools: loadout::retained(loaded, conversation),
            },
            Message::assistant()
                .with_text(continuation)
                .with_visibility(false, true)
                .into(),
        ];
        if let Some(message) = preserved {
            let mut carry = Message::user().with_visibility(false, true);
            carry.content = message
                .content
                .iter()
                .filter(|content| matches!(content, Content::Text { .. }))
                .cloned()
                .collect();
            effects.push(carry.into());
        }
        Ok(effects)
    }
    fn context_tokens(&self, messages: &[Message]) -> usize {
        let reported = messages
            .iter()
            .rposition(|message| message.metadata.usage.is_some());
        if let Some(index) = reported {
            let usage = messages[index].metadata.usage.as_deref().unwrap();
            if !usage.is_compaction
                && let Some(tokens) = usage.total
            {
                return tokens as usize + pending_tokens(&messages[index + 1..]);
            }
        }
        pending_tokens(messages)
    }
}

fn pending_tokens(messages: &[Message]) -> usize {
    let mut projected: Vec<_> = messages
        .iter()
        .filter(|message| message.is_agent_visible())
        .cloned()
        .collect();
    // Count the bounded payload that ordinary inference will send, rather than
    // triggering compaction for full canonical output the model will not receive.
    crate::agent::output::Cache::default().project(&mut projected);
    projected
        .iter()
        .map(|message| tokens::chat("", std::slice::from_ref(message)))
        .sum()
}
impl Compaction {
    pub(super) async fn run(
        &self,
        session: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let reactive =
            !self.manual && trailing_error(conversation) == Some(ErrorKind::ContextOverflow);
        if !self.manual {
            let messages = messages_since_kickoff(conversation)?;
            if reactive {
                let count = messages
                    .iter()
                    .filter(|message| {
                        message.error_kind() == Some(ErrorKind::ContextOverflow)
                            && !message.is_agent_visible()
                    })
                    .count();
                if count > 2 {
                    return not_applicable();
                }
            } else {
                if last_effective_role(messages)? == EffectiveRole::Assistant {
                    return not_applicable();
                }
                let tokens = self.context_tokens(conversation);
                if tokens == 0
                    || self.threshold <= 0.0
                    || self.threshold >= 1.0
                    || tokens as f64 / self.context as f64 <= self.threshold
                {
                    return not_applicable();
                }
            }
        }
        let mut input = conversation.to_vec();
        if reactive && let Some(last) = input.last_mut() {
            last.metadata.agent_visible = false;
        }
        let id = Message::assistant().with_generated_id().id.unwrap();
        if !self.manual {
            // Manual compaction already reports through its own run kind.
            let invocation = &self.preparer.invocation;
            if let Err(error) =
                self.preparer
                    .ingress
                    .compacting(invocation.turn.session, invocation.turn.id, &id)
            {
                tracing::warn!(error = %error.message, "compaction status could not be published");
            }
        }
        let result = tokio::select! {
            biased;
            _ = emit.cancelled() => return Ok(Outcome::Applied(Transition {
                effects: Vec::new(),
                status: RunStatus::Cancelled,
            })),
            result = self.compact(session, &input, &id) => result,
        };
        match result {
            Ok(effects) if self.manual => yielded_with(effects),
            Ok(effects) => applied(effects),
            Err(error) => yielded_with([
                emit.message(Message::assistant().with_error(
                    error
                        .downcast_ref::<Error>()
                        .map(|error| error.kind)
                        .unwrap_or(ErrorKind::Other),
                    format!(
                        "Ran into this error trying to compact: {error}.\n\nPlease try again or create a new session"
                    ),
                ))
                .into(),
            ]),
        }
    }
}
