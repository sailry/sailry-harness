//! Adapted from Goose ee8500d5, ops_compaction.rs and context_mgmt/mod.rs.
//! Apache-2.0; GDK owns summarization, rendering and context-overflow recovery.
use super::{Effect, Session, tokens};
use crate::agent::canonical::CompactedFile;
use async_trait::async_trait;
use goose_agent::operation::*;
use goose_context_management::{
    CompactionInput, CompactionModel, CompactionOutput, StructuredSummary, Templates,
    TokenEstimator,
};
use goose_provider_types::{
    base::{Provider, collect_stream},
    conversation::{
        Conversation, EffectiveRole,
        message::{Message, MessageContent, MessageErrorKind},
        token_usage::ProviderUsage,
    },
    errors::ProviderError,
    model::ModelConfig,
};
use std::sync::{Arc, Mutex};

mod input;
#[cfg(test)]
mod tests;

pub(super) struct Compaction {
    pub provider: Arc<dyn Provider>,
    pub config: ModelConfig,
    pub context: usize,
    pub threshold: f64,
    pub preparer: Arc<super::prompt::Preparer>,
    pub manual: bool,
}
/// Sailry's concise handoff prompt over GDK's structured summary contract.
const PROMPT: &str = include_str!("compact/prompt.md");
/// Renders the structured fields as Goal, Constraints, State, Decisions,
/// Next Steps and Essential Context.
const RENDER: &str = include_str!("compact/render.md");

struct Input(Vec<Message>);
impl CompactionInput for Input {
    fn messages(&self) -> Vec<Message> {
        self.0.clone()
    }
    fn templates(&self) -> Templates {
        Templates {
            compaction: PROMPT.into(),
            summary: RENDER.into(),
        }
    }
}

pub(super) struct Model {
    pub provider: Arc<dyn Provider>,
    pub config: ModelConfig,
    pub files: Mutex<Vec<CompactedFile>>,
}
#[async_trait]
impl CompactionModel for Model {
    /// Only a complete, parseable structured summary is accepted. A response
    /// cut off at the output limit or without the JSON document fails the
    /// compaction instead of replacing history with partial or raw text.
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        use futures::StreamExt;
        let limited = std::sync::atomic::AtomicBool::new(false);
        let stream = self
            .provider
            .stream(&self.config, system, messages, &[])
            .await?
            .inspect(|chunk| {
                if let Ok((Some(message), _)) = chunk
                    && message.metadata.output_token_limit_reached
                {
                    limited.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
        let (message, usage) = collect_stream(Box::pin(stream)).await?;
        if limited.load(std::sync::atomic::Ordering::Relaxed) || truncated(&usage) {
            return Err(ProviderError::ExecutionError(
                "compaction summary reached its output limit".into(),
            ));
        }
        let summary = StructuredSummary::parse(&message.as_concat_text()).ok_or_else(|| {
            ProviderError::ExecutionError("compaction summary was not a structured handoff".into())
        })?;
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
        Ok((message, usage))
    }
}

/// Provider stop reasons that mean the output limit ended the response.
fn truncated(usage: &ProviderUsage) -> bool {
    usage.finish_reasons.iter().flatten().any(|reason| {
        matches!(
            reason.to_ascii_lowercase().as_str(),
            "length" | "max_tokens" | "max_output_tokens"
        )
    })
}
struct Estimator;
#[async_trait]
impl TokenEstimator for Estimator {
    async fn count_chat_tokens(&self, system: &str, messages: &[Message]) -> usize {
        tokens::chat(system, messages)
    }
    async fn count_text_tokens(&self, text: &str) -> usize {
        tokens::text(text)
    }
}
#[derive(Default)]
struct Output {
    summary: Option<Message>,
    usage: Option<ProviderUsage>,
}
impl CompactionOutput for Output {
    fn set_summary(&mut self, summary: Message) {
        self.summary = Some(summary);
    }
    fn set_usage(&mut self, usage: ProviderUsage) {
        self.usage = Some(usage);
    }
}
impl Compaction {
    /// `id` is the summary record identity announced by the in-progress status.
    async fn compact(&self, conversation: &Conversation, id: &str) -> anyhow::Result<Vec<Effect>> {
        let model = Model {
            provider: self.provider.clone(),
            config: self.config.clone(),
            files: Mutex::default(),
        };
        let mut output = Output::default();
        let projected = Conversation::new_unvalidated(
            conversation
                .messages()
                .iter()
                .filter(|message| !message.is_turn_context())
                .cloned(),
        )
        .agent_visible_messages();
        let mut projected = Conversation::new_unvalidated(projected);
        self.preparer.hydrate(&mut projected).await?;
        let input = input::prepare(
            projected.messages(),
            self.context,
            self.config.max_tokens.unwrap_or(8_000).max(0) as usize,
        )?;
        goose_context_management::compact(&model, Some(&Estimator), &Input(input), &mut output)
            .await?;
        let summary = output
            .summary
            .ok_or_else(|| anyhow::anyhow!("compaction returned no message"))?
            .with_id(id.to_owned())
            .with_visibility(false, true);
        let usage = output
            .usage
            .ok_or_else(|| anyhow::anyhow!("compaction returned no usage"))?;
        let preserved = if self.manual {
            None
        } else {
            conversation.messages().iter().rev().find(|message| {
                message.is_agent_visible()
                    && !message.is_turn_context()
                    && message.role == rmcp::model::Role::User
                    && message
                        .content
                        .iter()
                        .any(|content| matches!(content, MessageContent::Text(_)))
                    && !message.content.iter().any(|content| {
                        matches!(
                            content,
                            MessageContent::ToolRequest(_) | MessageContent::ToolResponse(_)
                        )
                    })
            })
        };
        let is_latest = preserved.is_some_and(|message| {
            conversation
                .messages()
                .last()
                .and_then(|last| last.id.as_ref())
                == message.id.as_ref()
        });
        let continuation = if self.manual {
            "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else if is_latest {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nContinue calling tools as necessary to complete the task."
        };
        let mut effects = vec![
            Effect::Compaction {
                summary: Box::new(summary),
                usage,
                files: std::mem::take(&mut *model.files.lock().unwrap()),
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
                .filter(|content| matches!(content, MessageContent::Text(_)))
                .cloned()
                .collect();
            effects.push(carry.into());
        }
        Ok(effects)
    }
    fn context_tokens(&self, conversation: &Conversation) -> usize {
        let messages = conversation.messages();
        let reported = messages
            .iter()
            .rposition(|message| message.metadata.usage.is_some());
        if let Some(index) = reported {
            let usage = messages[index].metadata.usage.as_deref().unwrap();
            if !usage.is_compaction
                && let Some(tokens) = usage.total_tokens.filter(|value| *value >= 0)
            {
                return tokens as usize
                    + messages[index + 1..]
                        .iter()
                        .map(|message| tokens::chat("", std::slice::from_ref(message)))
                        .sum::<usize>();
            }
        }
        messages
            .iter()
            .filter(|message| message.is_agent_visible())
            .map(|message| tokens::chat("", std::slice::from_ref(message)))
            .sum()
    }
}
#[async_trait]
impl Operation<Session, Effect> for Compaction {
    fn name(&self) -> &'static str {
        "compaction"
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        if self.provider.manages_own_context() {
            return not_applicable();
        }
        let reactive = !self.manual
            && trailing_error(conversation) == Some(MessageErrorKind::ContextLengthExceeded);
        if !self.manual {
            let messages = messages_since_kickoff(conversation)?;
            if reactive {
                let count = messages
                    .iter()
                    .filter(|message| {
                        message.error_kind() == Some(MessageErrorKind::ContextLengthExceeded)
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
        let mut input = conversation.clone();
        if reactive && let Some(last) = input.messages_mut().last_mut() {
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
        match self.compact(&input, &id).await {
            Ok(effects) if self.manual => yielded_with(effects),
            Ok(effects) => applied(effects),
            Err(error) => yielded_with([
                emit.message(Message::assistant().with_error(
                    error
                        .downcast_ref::<ProviderError>()
                        .map(MessageErrorKind::from)
                        .unwrap_or(MessageErrorKind::Other),
                    format!(
                        "Ran into this error trying to compact: {error}.\n\nPlease try again or create a new session"
                    ),
                ))
                .into(),
            ]),
        }
    }
}
