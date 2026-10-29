//! Sailry's captured instructions and tool discovery enter GDK's prompt hooks.
//! Stable instructions form the system prompt so provider prompt caches survive
//! across requests; per-turn state follows the history as request-only context.
use super::{Effect, Session};
use crate::{
    agent::{
        attachments,
        canonical::BlockKind,
        model::observation::{Context, Source},
        prompt::instructions,
    },
    store::{Ingress, agent::Invocation},
};
use async_trait::async_trait;
use goose_agent::{
    inference::{InferenceRequestPreparer, PreparedInferenceRequest},
    operation::{InferenceInput, Operation},
};
use goose_provider_types::conversation::{
    Conversation,
    message::{Message, MessageMetadata},
};
use sailry_link::CancellationToken;
use sailry_protocol::conversation::observation::CategoryKind;
use std::{collections::BTreeSet, sync::Arc};

mod owners;

pub(super) struct Prompt {
    pub instruction: String,
    /// Turn-scoped title state and plugin script context.
    pub context: String,
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
            [("policy", &context.policy), ("personal", &context.personal)]
                .into_iter()
                .filter_map(|(name, text)| {
                    text.as_ref()
                        .filter(|text| !text.is_empty())
                        .map(|text| (name.into(), text.clone()))
                }),
        );
        if !context.guides.is_empty() {
            parts.push(("guides".into(), instructions::PREAMBLE.into()));
        }
        parts.extend(
            context
                .guides
                .iter()
                .map(|(key, guide)| (key.clone(), guide.content.clone())),
        );
        if let Some(text) = &session.capability {
            parts.push(("capabilities".into(), text.clone()));
        }
        Ok(parts)
    }

    async fn moim_parts(&self, session: &Session, _: &Conversation) -> anyhow::Result<Vec<String>> {
        let mut parts: Vec<_> = [session.context.environment.as_ref(), Some(&self.context)]
            .into_iter()
            .flatten()
            .filter(|text| !text.is_empty())
            .cloned()
            .collect();
        parts.extend(
            owners::collect(&session.records)
                .parts()
                .into_iter()
                .map(|(_, text)| text),
        );
        Ok(parts)
    }
}

#[cfg(test)]
pub(crate) mod input_evidence;

pub(super) struct Preparer {
    pub ingress: Arc<Ingress>,
    pub invocation: Arc<Invocation>,
    pub stop: CancellationToken,
    pub deferred: BTreeSet<String>,
    pub images: Vec<(Vec<String>, usize)>,
    pub sources: crate::agent::model::observation::Sources,
}
#[async_trait]
impl InferenceRequestPreparer<Session> for Preparer {
    async fn prepare_session(&self, session: &Session) -> anyhow::Result<Option<Session>> {
        let mut session = session.clone();
        self.hydrate(&mut session.conversation).await?;
        // Compaction hydrates without these owner prompt parts, so it retains
        // full skill results. Only ordinary inference receives the receipts.
        owners::collect(&session.records).project(session.conversation.messages_mut());
        Ok(Some(session))
    }
    async fn prepare(
        &self,
        session: &Session,
        _: &Conversation,
        input: InferenceInput,
    ) -> anyhow::Result<PreparedInferenceRequest> {
        let owners = owners::collect(&session.records).parts();
        self.sources.replace_with_context(
            input
                .prompt_parts
                .iter()
                .filter(|(name, _)| name != "instruction")
                .map(|(_, text)| Source {
                    kind: CategoryKind::Injected,
                    text: text.clone(),
                })
                .collect(),
            Context::new(
                input
                    .moim_parts
                    .iter()
                    .map(|text| Source {
                        kind: if owners
                            .iter()
                            .any(|(key, part)| key.starts_with("skill:") && part == text)
                        {
                            CategoryKind::Skills
                        } else {
                            CategoryKind::Injected
                        },
                        text: text.clone(),
                    })
                    .collect(),
            ),
        );
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
        Ok(prepare(input, &loaded, &self.deferred))
    }
}

impl Preparer {
    pub(super) async fn hydrate(
        &self,
        conversation: &mut Conversation,
    ) -> Result<(), sailry_protocol::Fault> {
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
            conversation,
            model.is_some_and(|model| model.vision),
            api,
            &self.stop,
        )
        .await?;
        crate::agent::media::screenshots::omit(conversation.messages_mut(), &self.images);
        crate::agent::output::project(conversation.messages_mut());
        Ok(())
    }
}

fn prepare(
    input: InferenceInput,
    loaded: &BTreeSet<String>,
    deferred: &BTreeSet<String>,
) -> PreparedInferenceRequest {
    let tools = input
        .tools
        .into_iter()
        .filter(|tool| {
            !deferred.contains(tool.name.as_ref()) || loaded.contains(tool.name.as_ref())
        })
        .collect();
    let parts: Vec<_> = input
        .prompt_parts
        .into_iter()
        .map(|(_, part)| part)
        .collect();
    PreparedInferenceRequest {
        system_prompt: parts.join("\n\n"),
        tools,
        additional_messages: context(&input.moim_parts).into_iter().collect(),
    }
}

/// Volatile turn state ends the request. GDK returns additional messages as
/// effects; the session writer drops this marked message, so it is never
/// persisted or repeated by later requests.
fn context(parts: &[String]) -> Option<Message> {
    Context::new(
        parts
            .iter()
            .map(|text| Source {
                kind: CategoryKind::Injected,
                text: text.clone(),
            })
            .collect(),
    )
    .map(|context| {
        Message::user()
            .with_text(context.text())
            .with_metadata(MessageMetadata::agent_only().with_turn_context())
    })
}

#[cfg(test)]
pub(crate) async fn prepared(session: &Session) -> PreparedInferenceRequest {
    let prompt = Prompt {
        instruction: "Fixture instructions".into(),
        context: String::new(),
    };
    prepare(
        InferenceInput {
            prompt_parts: prompt
                .prompt_parts(session, &session.conversation)
                .await
                .unwrap(),
            moim_parts: prompt
                .moim_parts(session, &session.conversation)
                .await
                .unwrap(),
            ..Default::default()
        },
        &BTreeSet::new(),
        &BTreeSet::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;

    #[test]
    fn filters_only_deferred_declarations() {
        let declarations = || InferenceInput {
            tools: ["always", "loaded", "deferred"]
                .map(|name| Tool::new(name, "Fixture", serde_json::Map::new()))
                .to_vec(),
            prompt_parts: vec![("owner".into(), "Current policy".into())],
            moim_parts: vec!["Current task".into()],
        };
        let deferred = ["loaded".into(), "deferred".into()].into();
        let request = prepare(declarations(), &["loaded".into()].into(), &deferred);
        assert_eq!(
            request
                .tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            ["always", "loaded"]
        );
        assert_eq!(request.system_prompt, "Current policy");
        let [context] = request.additional_messages.as_slice() else {
            panic!("one turn context expected")
        };
        assert!(context.is_turn_context());
        assert!(!context.is_user_visible());
        assert_eq!(
            context.as_concat_text(),
            "<turn-context>\nCurrent task\n</turn-context>"
        );
        let request = prepare(declarations(), &BTreeSet::new(), &deferred);
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "always");
    }
}
