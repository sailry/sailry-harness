//! Sailry's captured instructions and tool discovery enter GDK's prompt hooks.
//! Stable instructions form the system prompt so provider prompt caches survive
//! across requests; per-turn state is persisted as agent-only turn context.
use super::{Effect, Session};
use crate::{
    agent::{
        attachments,
        canonical::{BlockKind, Kind},
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
use goose_provider_types::conversation::Conversation;
use sailry_link::CancellationToken;
use sailry_protocol::conversation::observation::CategoryKind;
use std::{collections::BTreeSet, sync::Arc};

mod contexts;
mod owners;

pub(super) struct Prompt {
    pub instruction: String,
    /// Turn-scoped title state and plugin script context.
    pub context: String,
    /// Plugin parts, each persisted once while visible in the conversation.
    pub parts: Vec<String>,
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
        parts.extend(self.parts.iter().cloned());
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
    /// Plugin parts that `pending` skips once a persisted context holds them.
    pub parts: Vec<String>,
    pub sources: crate::agent::model::observation::Sources,
    /// Unavailable history warns once per attachment across this turn's requests.
    pub reported: attachments::Reported,
}
#[async_trait]
impl InferenceRequestPreparer<Session> for Preparer {
    async fn prepare_session(&self, session: &Session) -> anyhow::Result<Option<Session>> {
        let mut session = session.clone();
        self.hydrate(&mut session.conversation).await?;
        crate::agent::output::project(session.conversation.messages_mut());
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
        let retained = owners::collect(&session.records);
        let skills = retained.skill_parts();
        let owners = retained.parts();
        let owned: Vec<_> = owners
            .iter()
            .map(|(_, part)| part.as_str())
            .chain(self.parts.iter().map(String::as_str))
            .collect();
        let mut input = input;
        input.moim_parts =
            contexts::pending(conversation, &owned, std::mem::take(&mut input.moim_parts));
        let mut contexts: Vec<_> = conversation
            .messages()
            .iter()
            .filter(|message| message.is_turn_context() && message.is_agent_visible())
            .map(|message| Context::persisted(message.as_concat_text(), &skills))
            .collect();
        contexts.extend(Context::new(
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
        ));
        self.sources.replace_with_contexts(
            input
                .prompt_parts
                .iter()
                .filter(|(name, _)| name != "instruction")
                .map(|(_, text)| Source {
                    kind: CategoryKind::Injected,
                    text: text.clone(),
                })
                .collect(),
            contexts,
        );
        Ok(prepare(input, &loaded(session), &self.deferred, &owned))
    }
}

/// Calls before the latest compaction no longer keep their declarations; that
/// record already replaced the session's membership.
fn loaded(session: &Session) -> Vec<String> {
    let mut loaded = session.context.loaded_tools.clone().unwrap_or_default();
    let start = session
        .records
        .iter()
        .rposition(|record| matches!(record.kind, Kind::Compaction(_)))
        .map_or(0, |index| index + 1);
    for message in session.records[start..]
        .iter()
        .filter_map(|record| record.message())
    {
        for block in &message.content {
            if let BlockKind::ToolRequest(call) = &block.kind
                && !loaded.contains(&call.name)
            {
                loaded.push(call.name.clone());
            }
        }
    }
    loaded
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
            &self.invocation,
            &self.reported,
            conversation,
            model.is_some_and(|model| model.vision),
            api,
            &self.stop,
        )
        .await?;
        crate::agent::media::screenshots::omit(conversation.messages_mut(), &self.images);
        Ok(())
    }
}

fn prepare(
    input: InferenceInput,
    loaded: &[String],
    deferred: &BTreeSet<String>,
    owned: &[&str],
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
        additional_messages: contexts::message(&input.moim_parts, owned)
            .into_iter()
            .collect(),
    }
}

#[cfg(test)]
pub(crate) async fn prepared(session: &Session) -> PreparedInferenceRequest {
    let prompt = Prompt {
        instruction: "Fixture instructions".into(),
        context: String::new(),
        parts: Vec::new(),
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
        &[],
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
        let first = prepare(declarations(), &["zeta".into()], &deferred, &[]);
        let second = prepare(
            declarations(),
            &["zeta".into(), "alpha".into(), "zeta".into()],
            &deferred,
            &[],
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
        let request = prepare(declarations(), &["loaded".into()], &deferred, &[]);
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
        let request = prepare(declarations(), &[], &deferred, &[]);
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "always");
    }

    #[test]
    fn ignores_calls_before_compaction() {
        use crate::agent::canonical::{Block, Compaction, Message, Record, ToolRequest};
        let call = |name: &str| {
            let request = ToolRequest {
                id: format!("{name}-call"),
                name: name.into(),
                arguments: serde_json::json!({}),
                provider: None,
                tool_meta: None,
                error: None,
            };
            Record::new(
                sailry_protocol::TurnId::new(),
                Kind::Message(Message {
                    part_count: 1,
                    content: vec![Block {
                        index: 0,
                        kind: BlockKind::ToolRequest(request),
                    }],
                    ..Default::default()
                }),
            )
        };
        let now = chrono::Utc::now();
        let compaction = Record::new(
            sailry_protocol::TurnId::new(),
            Kind::Compaction(Compaction {
                boundary: "fixture-boundary".into(),
                summary: Message::default(),
                start: now,
                end: now,
                files: Vec::new(),
            }),
        );
        let mut session = super::super::tests::session(Vec::new());
        session.records = vec![call("alpha"), compaction, call("beta")];
        session.context.loaded_tools = Some(vec!["gamma".into()]);
        assert_eq!(loaded(&session), ["gamma", "beta"]);
    }
}
