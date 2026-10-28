//! Adapted from Goose ee8500d5, ops_compaction.rs and context_mgmt/mod.rs.
//! Apache-2.0; GDK owns summarization, rendering and context-overflow recovery.
use super::{Effect, Session, tokens};
use crate::agent::canonical::CompactedFile;
use async_trait::async_trait;
use goose_agent::operation::*;
use goose_context_management::{
    CompactionModel, CompactionOutput, ProviderModel, StructuredSummary, TokenEstimator,
};
use goose_provider_types::{
    base::Provider,
    conversation::{
        Conversation, EffectiveRole,
        message::{Message, MessageContent, MessageErrorKind},
        token_usage::ProviderUsage,
    },
    errors::ProviderError,
    model::ModelConfig,
};
use std::sync::{Arc, Mutex};

pub(super) struct Compaction {
    pub provider: Arc<dyn Provider>,
    pub config: ModelConfig,
    pub context: usize,
    pub threshold: f64,
}
struct Model {
    inner: ProviderModel,
    files: Mutex<Vec<CompactedFile>>,
}
#[async_trait]
impl CompactionModel for Model {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let (message, usage) = self.inner.complete(system, messages).await?;
        *self.files.lock().unwrap() = StructuredSummary::parse(&message.as_concat_text())
            .map(|summary| {
                summary
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
                    .collect()
            })
            .unwrap_or_default();
        Ok((message, usage))
    }
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
    pub(super) async fn compact(
        &self,
        conversation: &Conversation,
        manual: bool,
    ) -> anyhow::Result<Vec<Effect>> {
        let model = Model {
            inner: ProviderModel::new(self.provider.clone(), self.config.clone()),
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
        goose_context_management::compact(&model, Some(&Estimator), &projected, &mut output)
            .await?;
        let summary = output
            .summary
            .ok_or_else(|| anyhow::anyhow!("compaction returned no message"))?
            .with_visibility(false, true);
        let usage = output
            .usage
            .ok_or_else(|| anyhow::anyhow!("compaction returned no usage"))?;
        let preserved = if manual {
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
        let continuation = if manual {
            "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else if is_latest {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
        } else {
            "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nContinue calling tools as necessary to complete the task."
        };
        let mut effects = vec![
            Effect::Compaction {
                summary,
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
        let messages = messages_since_kickoff(conversation)?;
        let reactive =
            trailing_error(conversation) == Some(MessageErrorKind::ContextLengthExceeded);
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
        let mut input = conversation.clone();
        if reactive && let Some(last) = input.messages_mut().last_mut() {
            last.metadata.agent_visible = false;
        }
        match self.compact(&input,false).await {
            Ok(effects)=>applied(effects),
            Err(error)=>yielded_with([emit.message(Message::assistant().with_error(error.downcast_ref::<ProviderError>().map(MessageErrorKind::from).unwrap_or(MessageErrorKind::Other),format!("Ran into this error trying to compact: {error}.\n\nPlease try again or create a new session"))).into()]),
        }
    }
}
