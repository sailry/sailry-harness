//! Adapted from Goose ee8500d5, agents/state_machine/ops_tool_pair_compaction.rs
//! and context_mgmt/mod.rs (Apache-2.0, Copyright 2024 Block, Inc.).
//! Sailry's migration plan enables this optional upstream operation; providers
//! that manage their own context remain excluded. History effects keep one writer.
use super::{Effect, Session};
use async_trait::async_trait;
use goose_agent::operation::{
    ConversationEffect, Emitter, Operation, OperationResult, applied, messages_since_kickoff,
    not_applicable,
};
use goose_context_management::{DEFAULT_COMPACTION_THRESHOLD, format_message_for_compacting};
use goose_provider_types::{
    base::Provider,
    conversation::{
        Conversation,
        message::{Message, MessageContent, MessageMetadata},
    },
    model::ModelConfig,
    thinking::ThinkingEffort,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

const BATCH: usize = 10;

pub(super) struct ToolPairs {
    provider: Arc<dyn Provider>,
    config: ModelConfig,
    cutoff: usize,
    enabled: bool,
    output: Mutex<Vec<Effect>>,
}

impl ToolPairs {
    pub(super) fn new(
        provider: Arc<dyn Provider>,
        config: ModelConfig,
        context: usize,
        threshold: f64,
    ) -> Self {
        let enabled = !provider.manages_own_context();
        Self {
            provider,
            config: config
                .with_thinking_effort(ThinkingEffort::Off)
                .with_prompt_cache_disabled(),
            cutoff: cutoff(context, threshold),
            enabled,
            output: Mutex::default(),
        }
    }

    fn take_output(&self) -> Vec<Effect> {
        std::mem::take(&mut *self.output.lock().unwrap())
    }

    async fn summarize(&self, conversation: &Conversation, id: &str) -> anyhow::Result<Message> {
        let pair = visible_pair(conversation, id)?;
        let formatted = pair
            .iter()
            .map(format_message_for_compacting)
            .collect::<Vec<_>>()
            .join("\n");
        let system = "Your task is to summarize a tool call & response pair to save tokens.\n\nReply with a single message that describes what happened. Typically a tool call\nasks for something using a bunch of parameters and then the result is also some\nstructured output. So the tool might ask to look up something on github and the\nreply might be a json document. So you could reply with something like:\n\n\"A call to github was made to get the project status\"\n\nif that is what it was.\n";
        let (mut summary, _) = self
            .provider
            .complete(
                &self.config,
                system,
                &[Message::user().with_text(formatted)],
                &[],
            )
            .await?;
        summary.role = rmcp::model::Role::User;
        summary.created = pair.last().unwrap().created;
        summary.metadata = MessageMetadata::agent_only();
        Ok(summary.with_generated_id())
    }
}

#[async_trait]
impl Operation<Session, Effect> for ToolPairs {
    fn name(&self) -> &'static str {
        "tool_pair_compaction"
    }

    async fn finalize_cancellation(
        &self,
        _: &Session,
        _: &Conversation,
        _: &Emitter,
    ) -> Vec<Effect> {
        self.take_output()
    }

    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        _: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        if !self.enabled {
            return not_applicable();
        }
        let protected = messages_since_kickoff(conversation)?
            .iter()
            .flat_map(|message| &message.content)
            .filter(|content| matches!(content, MessageContent::ToolRequest(_)))
            .count();
        let ids = eligible(conversation, self.cutoff, protected);
        if ids.is_empty() {
            return not_applicable();
        }
        let mut hidden: HashSet<String> = HashSet::new();
        for id in ids {
            let pair: Vec<_> = conversation
                .messages()
                .iter()
                .filter(|message| {
                    message.id.is_some()
                        && message.content.iter().any(|content| match content {
                            MessageContent::ToolRequest(request) => request.id == id,
                            MessageContent::ToolResponse(response) => response.id == id,
                            _ => false,
                        })
                })
                .collect();
            if pair.len() != 2 {
                tracing::warn!(%id, messages=pair.len(), "Tool summary requires a request/response pair");
                continue;
            }
            // Parallel sibling calls share complete messages and must be hidden once.
            if pair
                .iter()
                .any(|message| message.id.as_ref().is_some_and(|id| hidden.contains(id)))
            {
                continue;
            }
            let requests: HashSet<&str> = pair
                .iter()
                .flat_map(|message| message.get_tool_request_ids())
                .collect();
            let responses: HashSet<&str> = pair
                .iter()
                .flat_map(|message| message.get_tool_response_ids())
                .collect();
            if requests != responses {
                tracing::warn!(%id, "Tool summary has siblings answered elsewhere");
                continue;
            }
            let summary = match self.summarize(conversation, &id).await {
                Ok(summary) => summary,
                Err(error) => {
                    tracing::warn!(%error, "Failed to summarize tool pair");
                    continue;
                }
            };
            let mut effects = self.output.lock().unwrap();
            for message in pair {
                let Some(message_id) = message.id.clone() else {
                    continue;
                };
                hidden.insert(message_id.clone());
                effects.push(
                    ConversationEffect::SetMessageVisibility {
                        message_id,
                        user_visible: message.is_user_visible(),
                        agent_visible: false,
                    }
                    .into(),
                );
            }
            effects.push(summary.into());
        }
        let effects = self.take_output();
        if effects.is_empty() {
            not_applicable()
        } else {
            applied(effects)
        }
    }
}

fn cutoff(context: usize, threshold: f64) -> usize {
    let threshold = if threshold > 0.0 && threshold <= 1.0 {
        threshold
    } else {
        DEFAULT_COMPACTION_THRESHOLD
    };
    let effective = (context as f64 * threshold) as usize;
    (3 * effective / 20_000).clamp(10, 500)
}

fn eligible(conversation: &Conversation, cutoff: usize, protected: usize) -> Vec<String> {
    let ids: Vec<_> = conversation
        .messages()
        .iter()
        .filter(|message| message.is_agent_visible())
        .flat_map(|message| &message.content)
        .filter_map(MessageContent::as_tool_request)
        .map(|request| request.id.clone())
        .collect();
    if ids.len().saturating_sub(protected) <= cutoff + BATCH {
        return Vec::new();
    }
    ids.into_iter().take(BATCH).collect()
}

fn visible_pair(conversation: &Conversation, id: &str) -> anyhow::Result<Vec<Message>> {
    let pair = Conversation::new_unvalidated(
        conversation
            .messages()
            .iter()
            .filter(|message| {
                message.content.iter().any(|content| match content {
                    MessageContent::ToolRequest(request) => request.id == id,
                    MessageContent::ToolResponse(response) => response.id == id,
                    _ => false,
                })
            })
            .cloned(),
    )
    .agent_visible_messages();
    if !pair.iter().any(|message| {
        message.content.iter().any(
            |content| matches!(content, MessageContent::ToolRequest(request) if request.id == id),
        )
    }) || !pair.iter().any(|message| {
        message.content.iter().any(|content| {
            matches!(content, MessageContent::ToolResponse(response) if response.id == id)
        })
    }) {
        return Err(anyhow::anyhow!(
            "No agent-visible tool pair found for id: {id}"
        ));
    }
    Ok(pair)
}

#[cfg(test)]
mod tests;
