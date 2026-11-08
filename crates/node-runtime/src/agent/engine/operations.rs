//! Adapted from Goose ee8500d5, agents/state_machine/ops_{tool_approval,
//! remaining_tool_calls,empty_response,exit_on_error,maxturns}.rs (Apache-2.0).
//! Durable approval belongs to the existing Node effect owner.
use super::{ConversationEffect, conversation::*, operation::*};
use super::{Effect, Session};
use crate::agent::model::Message;
use crate::{
    agent::canonical::{EXECUTABLE_KEY, ToolIdentity},
    store::agent::ApprovalBinding,
};
use rmcp::model::{CallToolResult, ContentBlock};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub(super) struct Approval {
    pub required: BTreeSet<String>,
    pub bindings: BTreeMap<String, ApprovalBinding>,
}
impl Approval {
    pub(super) async fn run(
        &self,
        _: &Session,
        conversation: &[Message],
        _: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let requests = pending(conversation)?;
        let mut allowed = Vec::new();
        for request in &requests {
            if request.error.is_some() {
                continue;
            }
            if self.required.contains(&request.name) || request_executable(request) == Some(true) {
                continue;
            }
            allowed.push(
                ConversationEffect::PatchToolRequestMeta {
                    tool_call_id: request.id.clone(),
                    patch: serde_json::json!({EXECUTABLE_KEY: true}),
                }
                .into(),
            );
        }
        if !allowed.is_empty() {
            return applied(allowed);
        }
        for request in requests {
            if request.error.is_some() {
                continue;
            }
            if !self.required.contains(&request.name) || request_executable(request) == Some(true) {
                continue;
            }
            return yielded_with([Effect::Approval {
                request: ToolIdentity {
                    id: Some(request.id.clone()),
                    name: request.name.clone(),
                    arguments: request.arguments.clone(),
                },
                binding: self.bindings.get(&request.name).cloned(),
            }]);
        }
        not_applicable()
    }
}

pub(super) struct PlanReview;
impl PlanReview {
    pub(super) async fn run(
        &self,
        session: &Session,
        _: &[Message],
        _: &Emitter,
    ) -> anyhow::Result<Outcome> {
        if session.reviewed {
            yielded()
        } else {
            not_applicable()
        }
    }
}

pub(super) struct Remaining {
    pub timeline: Option<std::sync::Arc<crate::agent::observation::Timeline>>,
}
impl Remaining {
    pub(super) async fn run(
        &self,
        _: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let requests = pending(conversation)?;
        if requests.is_empty() {
            return not_applicable();
        }
        let mut message = Message::user();
        for request in requests {
            if let Some(timeline) = &self.timeline {
                timeline.fail_tool(
                    "",
                    &request.id,
                    &request.name,
                    sailry_protocol::tool::Failure::runtime(None),
                    None,
                );
            }
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
impl EmptyResponse {
    pub(super) async fn run(
        &self,
        _: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        let Some(message) = messages_since_kickoff(conversation)?
            .last()
            .filter(|message| {
                message
                    .metadata
                    .operation_note("inference", "empty_response")
                    .is_some()
                    && !message.is_user_visible()
            })
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
impl ExitOnError {
    pub(super) async fn run(
        &self,
        _: &Session,
        conversation: &[Message],
        _: &Emitter,
    ) -> anyhow::Result<Outcome> {
        if trailing_error(conversation).is_some() {
            yielded()
        } else {
            not_applicable()
        }
    }
}

pub(super) struct MaxTurns(pub u32);
impl MaxTurns {
    pub(super) async fn contexts(&self, conversation: &[Message]) -> anyhow::Result<Vec<String>> {
        let used = assistant_turn_count(messages_since_kickoff(conversation)?);
        Ok((self.0 > 0 && used.saturating_mul(2) >= self.0)
            .then(|| format!("<turn-budget>{used}/{} used</turn-budget>", self.0))
            .into_iter()
            .collect())
    }
    pub(super) async fn run(
        &self,
        _: &Session,
        conversation: &[Message],
        emit: &Emitter,
    ) -> anyhow::Result<Outcome> {
        if assistant_turn_count(messages_since_kickoff(conversation)?) < self.0 {
            return not_applicable();
        }
        yielded_with([emit.message(Message::assistant().with_text("I've reached the maximum number of actions I can do without user input. Would you like me to continue?")).into()])
    }
}
