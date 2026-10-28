//! Goose reloads and effects use the existing Node storage worker.
use super::*;
use crate::agent::{
    canonical::*,
    engine::{Effect, history as conversion},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use goose_agent::{
    machine::{EffectHandler, MachineSession, SessionLoader},
    operation::{ConversationEffect, Emitter},
};
use goose_provider_types::{
    conversation::{
        Conversation,
        message::{Message as GdkMessage, MessageContent, MessageUsage},
        token_usage::ProviderUsage,
    },
    thinking::ThinkingEffort,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

pub(crate) const INPUT_TURN: &str = "sailry.context.input_turn";
pub(crate) const INPUT_SOURCE: &str = "sailry.context.input_source";
pub(crate) const INPUT_PART: &str = "sailry.context.input_part";
pub(crate) const CAPABILITY_PART: &str = "sailry.context.capability_part";

#[derive(Clone)]
pub(crate) struct Query {
    pub session: SessionId,
    pub recent: Option<usize>,
    pub after: Option<DateTime<Utc>>,
}

pub(crate) struct Sessions {
    ingress: Arc<Ingress>,
    turn: TurnId,
    input_turn: TurnId,
    input_source: &'static str,
    references: Vec<reference::Reference>,
    capability_part: Option<usize>,
    notices: Vec<sailry_protocol::conversation::background::Notice>,
    notice_input: Option<String>,
    presentation: Presentation,
    thinking: Option<ThinkingEffort>,
    invocation: Option<Arc<Invocation>>,
}

impl Sessions {
    pub(crate) fn new(ingress: Arc<Ingress>, turn: TurnId) -> Self {
        Self {
            ingress,
            turn,
            input_turn: turn,
            input_source: "user",
            references: Vec::new(),
            capability_part: None,
            notices: Vec::new(),
            notice_input: None,
            presentation: Presentation::default(),
            thinking: None,
            invocation: None,
        }
    }
    pub(crate) fn with_notices(
        mut self,
        notices: Vec<sailry_protocol::conversation::background::Notice>,
        input: Option<String>,
    ) -> Self {
        self.notices = notices;
        self.notice_input = input;
        self
    }
    pub(crate) fn with_references(mut self, references: Vec<reference::Reference>) -> Self {
        self.references = references;
        self
    }
    pub(crate) fn with_capability_part(mut self, part: Option<usize>) -> Self {
        self.capability_part = part;
        self
    }
    pub(crate) fn with_input_source(mut self, source: &'static str) -> Self {
        self.input_source = source;
        self
    }
    pub(crate) fn with_input_turn(mut self, turn: TurnId) -> Self {
        self.input_turn = turn;
        self
    }
    pub(crate) fn with_groupings(
        mut self,
        groupings: BTreeMap<String, sailry_protocol::tool::Grouping>,
    ) -> Self {
        self.presentation.groupings = groupings;
        self
    }
    pub(crate) fn with_presentations(
        mut self,
        tools: BTreeMap<String, sailry_protocol::tool::Presentation>,
    ) -> Self {
        self.presentation.tools = tools;
        self
    }
    pub(crate) fn with_displays(
        mut self,
        displays: BTreeMap<String, sailry_protocol::tool::Display>,
    ) -> Self {
        self.presentation.displays = displays;
        self
    }
    pub(crate) fn with_thinking(mut self, thinking: Option<ThinkingEffort>) -> Self {
        self.thinking = thinking;
        self
    }
    pub(crate) fn with_connection_context(
        self,
        _: BTreeMap<String, ApprovalBinding>,
        _: Vec<sailry_protocol::connection::Resource>,
    ) -> Self {
        self
    }

    pub(crate) fn with_invocation(mut self, invocation: Arc<Invocation>) -> Self {
        self.invocation = Some(invocation);
        self
    }

    pub(crate) async fn get(&self, query: Query) -> Result<Session, Fault> {
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(super::Operation::Get {
                turn: self.turn,
                request: query,
                reply,
            })))
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))?;
        response
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))?
    }

    pub(crate) async fn append_event(&self, session: &str, event: Record) -> Result<(), Fault> {
        self.append_with_context(session, event, None).await
    }

    pub(crate) async fn append_with_context(
        &self,
        session: &str,
        mut event: Record,
        context: Option<Record>,
    ) -> Result<(), Fault> {
        let session = session
            .parse()
            .map_err(|_| Fault::invalid("session identifier is invalid"))?;
        if event.provenance.author == "user" {
            event.turn = self.input_turn;
            let notice = self.notice_input.as_ref().is_some_and(|text| event.message().and_then(|message| message.content.first()).is_some_and(|block| matches!(&block.kind, BlockKind::Text { text: value } if value == text)));
            let source = match self.input_source {
                "parent" => InputSource::Parent,
                "plugin" => InputSource::Plugin,
                "notice" => InputSource::Notice,
                _ => InputSource::User,
            };
            let index = usize::from(!self.references.is_empty()) + usize::from(notice);
            if event.metadata.input.is_none() {
                event.metadata.input = Some(crate::agent::canonical::Input {
                    turn: Some(self.input_turn),
                    source: Some(source),
                    text_part: event
                        .message()
                        .and_then(|message| {
                            message.content.iter().find(|block| block.index == index)
                        })
                        .filter(|block| matches!(block.kind, BlockKind::Text { .. }))
                        .map(|_| index),
                    capability_part: self.capability_part,
                    notice_part: notice.then_some(0),
                    notice_ids: if notice {
                        self.notices
                            .iter()
                            .map(|notice| notice.id.clone())
                            .collect()
                    } else {
                        Vec::new()
                    },
                    references: self.references.clone(),
                });
            }
        }
        event
            .metadata
            .presentation
            .get_or_insert_with(|| self.presentation.clone());
        let turn = event.turn;
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(super::Operation::Append {
                session,
                turn,
                event,
                context: context.map(|event| (event.turn, event)),
                reply,
            })))
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))?;
        response
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))?
    }
}

#[derive(Clone)]
pub(crate) struct Session {
    pub id: String,
    pub records: Vec<Record>,
    pub conversation: Conversation,
    pub context: Context,
    pub state: HashMap<String, Value>,
    pub thinking: Option<ThinkingEffort>,
    pub invocation: Option<Arc<Invocation>>,
}

impl MachineSession for Session {
    fn id(&self) -> &str {
        &self.id
    }
    fn conversation(&self) -> Option<&Conversation> {
        Some(&self.conversation)
    }
    fn thinking_effort(&self) -> Option<ThinkingEffort> {
        self.thinking
    }
}

impl crate::agent::catalog::CatalogSession for Session {
    fn context(&self) -> &Context {
        &self.context
    }
}

#[async_trait]
impl SessionLoader<Session> for Sessions {
    async fn load(&self, session: &str) -> anyhow::Result<Session> {
        let mut session = self
            .get(Query {
                session: session.parse()?,
                recent: None,
                after: None,
            })
            .await?;
        session.thinking = self.thinking;
        session.invocation = self.invocation.clone();
        Ok(session)
    }
}

#[async_trait]
impl EffectHandler<Session, Effect> for Sessions {
    async fn apply_effects(
        &self,
        session: &Session,
        effects: &mut [Effect],
        _: &Emitter,
    ) -> anyhow::Result<()> {
        let mut pending = Vec::new();
        for effect in effects.iter() {
            if let Effect::Approval { request, binding } = effect {
                let decision = self
                    .ingress
                    .confirm(self.turn, request.clone(), binding.clone())
                    .await?;
                match decision {
                    Decision::Approve | Decision::ApproveSession | Decision::ApprovePrefix(_) => {
                        pending.push(Effect::from(ConversationEffect::PatchToolRequestMeta {
                            tool_call_id: request
                                .id
                                .clone()
                                .ok_or_else(|| Fault::invalid("tool call identity is missing"))?,
                            patch: json!({"goose.executable":true}),
                        }))
                    }
                    Decision::Deny | Decision::DenyWithReason(_) => {
                        let message = GdkMessage::user().with_generated_id().with_tool_response(
                            request
                                .id
                                .clone()
                                .ok_or_else(|| Fault::invalid("tool call identity is missing"))?,
                            Ok(rmcp::model::CallToolResult::error(vec![
                                rmcp::model::ContentBlock::text("Tool execution was denied."),
                            ])),
                        );
                        pending.push(Effect::from(message));
                    }
                }
            } else {
                pending.push(effect.clone());
            }
        }
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(super::Operation::Effects {
                session: session.id.parse()?,
                turn: self.turn,
                effects: pending,
                presentation: self.presentation.clone(),
                reply,
            })))
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))?;
        response
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Node storage is closed"))??;
        Ok(())
    }
}

pub(super) async fn get(db: &Connection, turn: TurnId, query: Query) -> Result<Session, Fault> {
    check_session(db, query.session).await?;
    let owns: bool = db
        .query(
            "SELECT 1 FROM turns WHERE id=?1 AND session=?2",
            params![turn.to_string(), query.session.to_string()],
        )
        .await
        .map_err(storage_error)?
        .next()
        .await
        .map_err(storage_error)?
        .is_some();
    if !owns {
        return Err(Fault::invalid("session does not belong to this invocation"));
    }
    let mut rows = db.query("SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence",[query.session.to_string()]).await.map_err(storage_error)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body: Vec<u8> = row.get(0).map_err(storage_error)?;
        let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
        if query.after.is_none_or(|after| record.created >= after) {
            records.push(record);
        }
    }
    if let Some(recent) = query.recent {
        records = records.into_iter().rev().take(recent).collect();
        records.reverse();
    }
    let mut messages = Vec::new();
    for record in &records {
        if matches!(&record.kind, Kind::Compaction(_)) {
            for message in &mut messages {
                message.metadata.agent_visible = false;
            }
        }
        if let Some(message) = conversion::project(db, record).await? {
            messages.push(message);
        }
        if let Kind::Operation(operation) = &record.kind {
            apply_operation(&mut messages, operation);
        }
    }
    let state = history::state(db, query.session).await?;
    Ok(Session {
        id: query.session.to_string(),
        records,
        conversation: Conversation::new_unvalidated(messages),
        context: Context::from_state(&state),
        state,
        thinking: None,
        invocation: None,
    })
}

fn apply_operation(messages: &mut [GdkMessage], operation: &crate::agent::canonical::Operation) {
    use crate::agent::canonical::Operation as Change;
    match operation {
        Change::Visibility {
            message,
            user_visible,
            agent_visible,
        } => {
            if let Some(found) = messages
                .iter_mut()
                .find(|value| value.id.as_ref() == Some(message))
            {
                found.metadata.user_visible = *user_visible;
                found.metadata.agent_visible = *agent_visible;
            }
        }
        Change::ToolMeta { call, patch } => {
            for content in messages.iter_mut().flat_map(|message| &mut message.content) {
                if let MessageContent::ToolRequest(request) = content
                    && request.id == *call
                {
                    let meta = request.tool_meta.get_or_insert_with(|| json!({}));
                    if let (Some(meta), Some(patch)) = (meta.as_object_mut(), patch.as_object()) {
                        meta.extend(patch.clone());
                    }
                }
            }
        }
        Change::Note { message, note } => {
            if let Some(found) = messages
                .iter_mut()
                .find(|value| value.id.as_ref() == Some(message))
            {
                match note {
                    OperationNote::EmptyResponse => found.metadata.set_operation_note(
                        "inference",
                        "empty_response",
                        json!(true),
                    ),
                    OperationNote::SubagentDelivered { session } => found
                        .metadata
                        .set_operation_note("foreground_subagent", "delivered", json!(session)),
                    OperationNote::SubagentsCancelled { sessions } => found
                        .metadata
                        .set_operation_note("foreground_subagent", "cancelled", json!(sessions)),
                }
            }
        }
    }
}

pub(super) async fn effects(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    effects: Vec<Effect>,
    presentation: Presentation,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let current = get(
        &database.connection,
        turn,
        Query {
            session,
            recent: None,
            after: None,
        },
    )
    .await?;
    let mut calls = BTreeMap::new();
    for content in current
        .conversation
        .messages()
        .iter()
        .flat_map(|message| &message.content)
    {
        if let MessageContent::ToolRequest(request) = content
            && let Ok(call) = &request.tool_call
        {
            calls.insert(request.id.clone(), call.name.to_string());
        }
    }
    let mut usage = None;
    for effect in effects {
        match effect {
            Effect::Usage(value) => usage = Some(value),
            Effect::Records(records) => {
                for mut record in records {
                    record.metadata.presentation = Some(presentation.clone());
                    history::append(database, session, record.turn, record, events).await?;
                }
            }
            Effect::Approval { .. } => {
                unreachable!("approval is resolved before submitting storage effects")
            }
            Effect::Conversation(ConversationEffect::AppendMessage(mut message)) => {
                if let Some(value) = usage.take() {
                    message.metadata.usage =
                        Some(Box::new(MessageUsage::from_provider_usage(&value, false)));
                }
                for content in &message.content {
                    if let MessageContent::ToolRequest(request) = content
                        && let Ok(call) = &request.tool_call
                    {
                        calls.insert(request.id.clone(), call.name.to_string());
                    }
                }
                history::append_message(
                    database,
                    session,
                    turn,
                    message,
                    calls.clone(),
                    presentation.clone(),
                    events,
                )
                .await?;
            }
            Effect::Compaction {
                summary,
                usage,
                files,
            } => save_summary(database, session, turn, summary, usage, files, events).await?,
            Effect::Conversation(ConversationEffect::ReplaceConversation(_)) => {
                return Err(Fault::invalid(
                    "conversation replacement requires a compaction checkpoint",
                ));
            }
            Effect::Conversation(effect) => {
                let operation = match effect {
                    ConversationEffect::SetMessageVisibility {
                        message_id,
                        user_visible,
                        agent_visible,
                    } => crate::agent::canonical::Operation::Visibility {
                        message: message_id,
                        user_visible,
                        agent_visible,
                    },
                    ConversationEffect::PatchToolRequestMeta {
                        tool_call_id,
                        patch,
                    } => crate::agent::canonical::Operation::ToolMeta {
                        call: tool_call_id,
                        patch,
                    },
                    ConversationEffect::SetMessageOperationNote {
                        message_id,
                        operation,
                        key,
                        value,
                    } => {
                        let note = match (operation.as_str(), key.as_str()) {
                            ("inference", "empty_response") => OperationNote::EmptyResponse,
                            ("foreground_subagent", "delivered") => {
                                OperationNote::SubagentDelivered {
                                    session: serde_json::from_value(value)
                                        .map_err(storage_error)?,
                                }
                            }
                            ("foreground_subagent", "cancelled") => {
                                OperationNote::SubagentsCancelled {
                                    sessions: serde_json::from_value(value)
                                        .map_err(storage_error)?,
                                }
                            }
                            _ => return Err(Fault::invalid("operation note is unsupported")),
                        };
                        crate::agent::canonical::Operation::Note {
                            message: message_id,
                            note,
                        }
                    }
                    _ => unreachable!("append and replace handled above"),
                };
                let mut record = Record::new(turn, Kind::Operation(operation));
                record.provenance.author = "system".into();
                record.metadata.runtime = Some(Runtime {
                    user_visible: false,
                    agent_visible: false,
                    ..Runtime::default()
                });
                history::append(database, session, turn, record, events).await?;
            }
        }
    }
    Ok(())
}

async fn save_summary(
    database: &mut Database,
    session: SessionId,
    turn: TurnId,
    mut summary: GdkMessage,
    usage: ProviderUsage,
    files: Vec<CompactedFile>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let current = get(
        &database.connection,
        turn,
        Query {
            session,
            recent: None,
            after: None,
        },
    )
    .await?;
    let Some(boundary) = current
        .records
        .iter()
        .rev()
        .find(|record| matches!(record.kind, Kind::Message(_)))
    else {
        return Err(Fault::invalid("compaction history is empty"));
    };
    summary.metadata.usage = Some(Box::new(MessageUsage::from_provider_usage(&usage, true)));
    history::append_summary(
        database,
        session,
        turn,
        summary,
        boundary.id.clone(),
        current
            .records
            .first()
            .map_or(boundary.created, |record| record.created),
        boundary.created,
        files,
        events,
    )
    .await
}
