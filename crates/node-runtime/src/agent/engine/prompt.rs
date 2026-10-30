//! Sailry's captured instructions and tool discovery enter GDK's prompt hooks.
//! Stable instructions form the system prompt so provider prompt caches survive
//! across requests; per-turn state is persisted as agent-only turn context.
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
    operation::{InferenceInput, Operation, messages_since_kickoff},
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
        conversation: &Conversation,
        input: InferenceInput,
    ) -> anyhow::Result<PreparedInferenceRequest> {
        let owners = owners::collect(&session.records).parts();
        let owned: Vec<_> = owners.iter().map(|(_, part)| part.as_str()).collect();
        let mut input = input;
        input.moim_parts = pending(conversation, &owned, std::mem::take(&mut input.moim_parts));
        let context = Context::new(
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
        );
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
            context,
        );
        let mut loaded = session.context.loaded_tools.clone().unwrap_or_default();
        for record in &session.records {
            if let Some(message) = record.message() {
                for block in &message.content {
                    if let BlockKind::ToolRequest(call) = &block.kind
                        && !loaded.contains(&call.name)
                    {
                        loaded.push(call.name.clone());
                    }
                }
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
    loaded: &[String],
    deferred: &BTreeSet<String>,
) -> PreparedInferenceRequest {
    let mut tools = Vec::new();
    let mut additional = std::collections::BTreeMap::new();
    for tool in input.tools {
        if deferred.contains(tool.name.as_ref()) {
            additional.insert(tool.name.to_string(), tool);
        } else {
            tools.push(tool);
        }
    }
    for name in loaded {
        if let Some(tool) = additional.remove(name) {
            tools.push(tool);
        }
    }
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

/// Parts for a new persisted turn context, or none when history already holds
/// them. As in Goose, each turn states the session once and repeats it only when
/// it changes, so each request extends the previous one. Retained owner parts
/// repeat only after compaction hides their earlier copy.
fn pending(conversation: &Conversation, owned: &[&str], parts: Vec<String>) -> Vec<String> {
    let persisted = |message: &&Message| message.is_turn_context() && message.is_agent_visible();
    let visible: Vec<_> = conversation
        .messages()
        .iter()
        .filter(persisted)
        .map(Message::as_concat_text)
        .collect();
    let last = messages_since_kickoff(conversation)
        .ok()
        .and_then(|turn| turn.iter().rev().find(persisted))
        .map(Message::as_concat_text);
    let parts: Vec<_> = parts
        .into_iter()
        .filter(|part| {
            !owned.contains(&part.as_str())
                || !visible.iter().any(|text| text.contains(part.as_str()))
        })
        .collect();
    let Some(candidate) = context(&parts).map(|message| message.as_concat_text()) else {
        return Vec::new();
    };
    let changed = last.is_none_or(|last| state(&last) != state(&candidate));
    if changed || parts.iter().any(|part| owned.contains(&part.as_str())) {
        parts
    } else {
        Vec::new()
    }
}

/// Session state of a context, without its retained owner parts.
fn state(text: &str) -> &str {
    let text = text.strip_suffix("\n</turn-context>").unwrap_or(text);
    let end = text
        .find(&format!("\n{}", owners::PREFIX))
        .unwrap_or(text.len());
    text[..end].trim_end_matches('\n')
}

/// Turn state as an agent-only message. GDK returns it as an effect and the
/// canonical writer persists it, so later requests repeat the same bytes.
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
        &[],
        &BTreeSet::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Tool;

    #[test]
    fn appends_discoveries_after_existing_declarations() {
        let declarations = || InferenceInput {
            tools: ["alpha", "base", "tool_search", "zeta"]
                .map(|name| Tool::new(name, "Fixture", serde_json::Map::new()))
                .to_vec(),
            ..Default::default()
        };
        let deferred = ["alpha".into(), "zeta".into()].into();
        let first = prepare(declarations(), &["zeta".into()], &deferred);
        let second = prepare(
            declarations(),
            &["zeta".into(), "alpha".into(), "zeta".into()],
            &deferred,
        );
        assert_eq!(second.tools[..first.tools.len()], first.tools);
        assert_eq!(
            second
                .tools
                .iter()
                .map(|tool| tool.name.as_ref())
                .collect::<Vec<_>>(),
            ["base", "tool_search", "zeta", "alpha"]
        );
    }

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
        let request = prepare(declarations(), &["loaded".into()], &deferred);
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
        let request = prepare(declarations(), &[], &deferred);
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "always");
    }

    mod contexts {
        use super::*;

        const ENVIRONMENT: &str = "Session environment";
        const SKILL: &str = "Retained owner context: skill:fixture:SKILL.md\nBody";
        const PLAN: &str = "Retained owner context: plan\nSteps";

        fn persisted(parts: &[&str]) -> Message {
            context(
                &parts
                    .iter()
                    .map(|part| (*part).to_owned())
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        }

        fn select(messages: Vec<Message>, parts: &[&str]) -> Vec<String> {
            pending(
                &Conversation::new_unvalidated(messages),
                &[SKILL, PLAN],
                parts.iter().map(|part| (*part).to_owned()).collect(),
            )
        }

        #[test]
        fn starts_with_every_part() {
            assert_eq!(
                select(
                    vec![Message::user().with_text("Task")],
                    &[ENVIRONMENT, SKILL]
                ),
                [ENVIRONMENT, SKILL]
            );
            assert!(select(vec![Message::user().with_text("Task")], &[]).is_empty());
        }

        #[test]
        fn skips_unchanged_turn() {
            let history = vec![
                Message::user().with_text("Task"),
                persisted(&[ENVIRONMENT, SKILL]),
                Message::assistant().with_text("Working"),
            ];
            assert!(select(history.clone(), &[ENVIRONMENT, SKILL]).is_empty());
            assert_eq!(
                select(history.clone(), &["Moved environment", SKILL]),
                ["Moved environment"]
            );
            assert_eq!(
                select(history, &[ENVIRONMENT, SKILL, PLAN]),
                [ENVIRONMENT, PLAN]
            );
        }

        #[test]
        fn states_each_turn_once() {
            let history = vec![
                Message::user().with_text("Task"),
                persisted(&[ENVIRONMENT, SKILL]),
                Message::assistant().with_text("Done"),
                Message::user().with_text("Next task"),
            ];
            assert_eq!(select(history, &[ENVIRONMENT, SKILL]), [ENVIRONMENT]);
        }

        #[test]
        fn keeps_state_beside_owner_parts() {
            let history = vec![
                Message::user().with_text("Task"),
                persisted(&[ENVIRONMENT, SKILL]),
                persisted(&[ENVIRONMENT, PLAN]),
            ];
            assert!(select(history, &[ENVIRONMENT, SKILL, PLAN]).is_empty());
            let owners = vec![Message::user().with_text("Task"), persisted(&[SKILL])];
            assert!(select(owners.clone(), &[SKILL]).is_empty());
            assert_eq!(select(owners, &[ENVIRONMENT, SKILL]), [ENVIRONMENT]);
        }

        #[test]
        fn restores_hidden_owner_parts() {
            let mut hidden = persisted(&[ENVIRONMENT, SKILL]);
            hidden.metadata = hidden.metadata.with_agent_invisible();
            assert_eq!(
                select(
                    vec![Message::user().with_text("Task"), hidden],
                    &[ENVIRONMENT, SKILL]
                ),
                [ENVIRONMENT, SKILL]
            );
        }
    }
}
