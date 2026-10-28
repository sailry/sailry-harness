//! Adapted from Goose ee8500d5, agents/state_machine/ops_{tool_approval,
//! remaining_tool_calls,empty_response,exit_on_error,maxturns}.rs (Apache-2.0).
//! Durable approval belongs to the existing Node effect owner.
use super::{Effect, Session};
use crate::{agent::canonical::ToolIdentity, store::agent::ApprovalBinding};
use async_trait::async_trait;
use goose_agent::{inference::is_empty_response_marker, operation::*};
use goose_provider_types::conversation::{
    Conversation,
    message::{Message, MessageContent, ToolRequest},
};
use rmcp::model::{CallToolResult, ContentBlock};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn pending(conversation: &Conversation) -> anyhow::Result<Vec<&ToolRequest>> {
    let messages = messages_since_kickoff(conversation)?;
    let answered: BTreeSet<_> = messages
        .iter()
        .flat_map(Message::get_tool_response_ids)
        .collect();
    Ok(messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(MessageContent::as_tool_request)
        .filter(|request| {
            !request.was_executed_externally() && !answered.contains(request.id.as_str())
        })
        .collect())
}

pub(super) struct Approval {
    pub required: BTreeSet<String>,
    pub bindings: BTreeMap<String, ApprovalBinding>,
}
#[async_trait]
impl Operation<Session, Effect> for Approval {
    fn name(&self) -> &'static str {
        "tool_approval"
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        for request in pending(conversation)? {
            let Ok(call) = &request.tool_call else {
                continue;
            };
            if !self.required.contains(call.name.as_ref())
                || request
                    .tool_meta
                    .as_ref()
                    .and_then(|value| value.get("goose.executable"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                continue;
            }
            let action = Message::assistant()
                .with_action_required(
                    request.id.clone(),
                    call.name.to_string(),
                    call.arguments.clone().unwrap_or_default(),
                    None,
                )
                .user_only();
            emit.message(action);
            return yielded_with([Effect::Approval {
                request: ToolIdentity {
                    id: Some(request.id.clone()),
                    name: call.name.to_string(),
                    arguments: serde_json::Value::Object(
                        call.arguments.clone().unwrap_or_default(),
                    ),
                },
                binding: self.bindings.get(call.name.as_ref()).cloned(),
            }]);
        }
        not_applicable()
    }
}

pub(super) struct Remaining;
#[async_trait]
impl Operation<Session, Effect> for Remaining {
    fn name(&self) -> &'static str {
        "remaining_tool_calls"
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        let requests = pending(conversation)?;
        if requests.is_empty() {
            return not_applicable();
        }
        let mut message = Message::user();
        for request in requests {
            message.add_tool_response_with_metadata(
                request.id.clone(),
                Ok(CallToolResult::error(vec![ContentBlock::text(
                    "This tool call was not handled.",
                )])),
                request.metadata.as_ref(),
            );
        }
        applied([emit.message(message).into()])
    }
}

pub(super) struct EmptyResponse;
#[async_trait]
impl Operation<Session, Effect> for EmptyResponse {
    fn name(&self) -> &'static str {
        "empty_response"
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        let Some(message) = messages_since_kickoff(conversation)?
            .last()
            .filter(|message| is_empty_response_marker(message) && !message.is_user_visible())
        else {
            return not_applicable();
        };
        let id = message
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("empty-response message has no identity"))?;
        emit.message(message.clone().with_visibility(true, true));
        applied([ConversationEffect::SetMessageVisibility {
            message_id: id,
            user_visible: true,
            agent_visible: true,
        }
        .into()])
    }
}

pub(super) struct ExitOnError;
#[async_trait]
impl Operation<Session, Effect> for ExitOnError {
    fn name(&self) -> &'static str {
        "exit_on_error"
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        _: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        if trailing_error(conversation).is_some() {
            yielded()
        } else {
            not_applicable()
        }
    }
}

pub(super) struct MaxTurns(pub u32);
#[async_trait]
impl Operation<Session, Effect> for MaxTurns {
    fn name(&self) -> &'static str {
        "max_turns"
    }
    async fn moim_parts(
        &self,
        _: &Session,
        conversation: &Conversation,
    ) -> anyhow::Result<Vec<String>> {
        let used = assistant_turn_count(messages_since_kickoff(conversation)?);
        Ok((self.0 > 0 && used.saturating_mul(2) >= self.0)
            .then(|| format!("<turn-budget>{used}/{} used</turn-budget>", self.0))
            .into_iter()
            .collect())
    }
    async fn run(
        &self,
        _: &Session,
        conversation: &Conversation,
        emit: &Emitter,
    ) -> anyhow::Result<OperationResult<Effect>> {
        if assistant_turn_count(messages_since_kickoff(conversation)?) < self.0 {
            return not_applicable();
        }
        yielded_with([emit.message(Message::assistant().with_text("I've reached the maximum number of actions I can do without user input. Would you like me to continue?")).into()])
    }
}
