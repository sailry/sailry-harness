//! Sailry's captured instructions and tool discovery enter GDK's prompt hooks.
use super::{Effect, Session};
use crate::{
    agent::{attachments, canonical::BlockKind},
    store::{Ingress, agent::Invocation},
};
use async_trait::async_trait;
use goose_agent::{
    inference::{InferenceRequestPreparer, PreparedInferenceRequest},
    operation::{InferenceInput, Operation},
};
use goose_provider_types::conversation::Conversation;
use sailry_link::CancellationToken;
use std::{collections::BTreeSet, sync::Arc};

pub(super) struct Prompt {
    pub instruction: String,
}
#[async_trait]
impl Operation<Session, Effect> for Prompt {
    fn name(&self) -> &'static str {
        "prompt"
    }
    async fn prompt_parts(
        &self,
        session: &Session,
        _: &Conversation,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let context = &session.context;
        let mut parts = vec![("instruction".into(), self.instruction.clone())];
        parts.extend(
            [
                ("policy", &context.policy),
                ("environment", &context.environment),
                ("personal", &context.personal),
            ]
            .into_iter()
            .filter_map(|(name, text)| text.as_ref().map(|text| (name.into(), text.clone()))),
        );
        parts.extend(
            context
                .guides
                .iter()
                .map(|(key, guide)| (key.clone(), guide.content.clone())),
        );
        Ok(parts)
    }
}

pub(super) struct Preparer {
    pub ingress: Arc<Ingress>,
    pub invocation: Arc<Invocation>,
    pub stop: CancellationToken,
    pub deferred: BTreeSet<String>,
    pub images: Vec<(Vec<String>, usize)>,
}
#[async_trait]
impl InferenceRequestPreparer<Session> for Preparer {
    async fn prepare_session(&self, session: &Session) -> anyhow::Result<Option<Session>> {
        let mut session = session.clone();
        let model = self.invocation.provider.as_ref().and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.id == self.invocation.turn.config.model)
        });
        let api = self
            .invocation
            .provider
            .as_ref()
            .map(|provider| provider.api)
            .unwrap_or(sailry_protocol::conversation::ModelApi::Responses);
        attachments::hydrate(
            &self.ingress,
            self.invocation.turn.id,
            &mut session.conversation,
            model.is_some_and(|model| model.vision),
            api,
            &self.stop,
        )
        .await?;
        crate::agent::media::screenshots::omit(session.conversation.messages_mut(), &self.images);
        Ok(Some(session))
    }
    async fn prepare(
        &self,
        session: &Session,
        conversation: &Conversation,
        input: InferenceInput,
    ) -> anyhow::Result<PreparedInferenceRequest> {
        let mut loaded = session.context.loaded_tools.clone().unwrap_or_default();
        for record in &session.records {
            if let Some(message) = record.message() {
                loaded.extend(
                    message
                        .content
                        .iter()
                        .filter_map(|block| match &block.kind {
                            BlockKind::ToolRequest(call) => Some(call.name.clone()),
                            _ => None,
                        }),
                );
            }
        }
        let tools = input
            .tools
            .into_iter()
            .filter(|tool| {
                !self.deferred.contains(tool.name.as_ref()) || loaded.contains(tool.name.as_ref())
            })
            .collect();
        let mut parts: Vec<_> = input
            .prompt_parts
            .into_iter()
            .map(|(_, part)| part)
            .collect();
        parts.extend(input.moim_parts);
        let _ = conversation;
        Ok(PreparedInferenceRequest {
            system_prompt: parts.join("\n\n"),
            tools,
            additional_messages: Vec::new(),
        })
    }
}
