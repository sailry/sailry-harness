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
use std::collections::BTreeMap;
mod presentation;

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
    bindings: BTreeMap<String, ApprovalBinding>,
    connections: Vec<sailry_protocol::connection::Resource>,
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
            bindings: BTreeMap::new(),
            connections: Vec::new(),
        }
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
        mut self,
        bindings: BTreeMap<String, ApprovalBinding>,
        connections: Vec<sailry_protocol::connection::Resource>,
    ) -> Self {
        self.bindings = bindings;
        self.connections = connections;
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
        if event.metadata.presentation.is_none() {
            let calls = event
                .message()
                .into_iter()
                .flat_map(|message| &message.content)
                .filter_map(|block| match &block.kind {
                    BlockKind::ToolRequest(request) => {
                        Some((request.name.clone(), request.arguments.clone()))
                    }
                    _ => None,
                })
                .collect();
            let captured = self.current_presentation(calls).await?;
            super::presentation::capture_failures(
                &mut event,
                self.ingress.agents.timeline(self.turn).as_deref(),
            );
            super::presentation::stamp(&mut event, &captured);
        }
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
    pub capability: Option<String>,
    pub reviewed: bool,
    #[cfg(test)]
    pub state: std::collections::HashMap<String, Value>,
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
                session: session.parse().map_err(anyhow::Error::msg)?,
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
                let stop = self
                    .ingress
                    .agents
                    .runner(self.turn)
                    .unwrap_or_else(|| self.ingress.agents.stopped.child_token());
                let decision = tokio::select! {biased;_=stop.cancelled()=>return Err(Fault::new(ErrorCode::Cancelled,"tool approval was cancelled").into()),decision=self.ingress.confirm(self.turn,request.clone(),binding.clone())=>decision?};
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
                        let reason = match decision {
                            Decision::DenyWithReason(reason) => {
                                format!("Tool execution was denied. Reason: {reason}")
                            }
                            _ => "Tool execution was denied.".to_owned(),
                        };
                        let message = GdkMessage::user().with_generated_id().with_tool_response(
                            request
                                .id
                                .clone()
                                .ok_or_else(|| Fault::invalid("tool call identity is missing"))?,
                            Ok(rmcp::model::CallToolResult::error(vec![
                                rmcp::model::ContentBlock::text(reason),
                            ])),
                        );
                        pending.push(Effect::from(message));
                    }
                }
            } else {
                pending.push(effect.clone());
            }
        }
        let calls = pending
            .iter()
            .flat_map(|effect| match effect {
                Effect::Conversation(ConversationEffect::AppendMessage(message)) => message
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        MessageContent::ToolRequest(request) => {
                            request.tool_call.as_ref().ok().map(|call| {
                                (
                                    call.name.to_string(),
                                    Value::Object(call.arguments.clone().unwrap_or_default()),
                                )
                            })
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect();
        let presentation = self.current_presentation(calls).await?;
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(super::Operation::Effects {
                session: session.id.parse().map_err(anyhow::Error::msg)?,
                turn: self.turn,
                effects: pending,
                presentation,
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
    let mut messages: Vec<GdkMessage> = Vec::new();
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
        capability: capability(db, query.session).await?,
        reviewed: super::questions::reviewed(db, turn).await?,
        #[cfg(test)]
        state,
        thinking: None,
        invocation: None,
    })
}

async fn capability(db: &Connection, session: SessionId) -> Result<Option<String>, Fault> {
    // Guidance shares its admitted execution owner; notices do not replace that owner.
    let mut query = db.prepare(
        "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         JOIN agent_runs selected_run ON selected_run.turn=e.turn
         WHERE h.session=?1 AND json_extract(e.body,'$.kind')='message'
           AND json_type(e.body,'$.metadata.input.capability_part')='integer'
           AND coalesce(json_extract(selected_run.body,'$.joined'),selected_run.turn)=(
             SELECT coalesce(json_extract(latest_run.body,'$.joined'),latest_run.turn)
             FROM conversation_events latest JOIN agent_events input ON input.sequence=latest.sequence
             JOIN agent_runs latest_run ON latest_run.turn=input.turn
             WHERE latest.session=?1 AND json_extract(input.body,'$.kind')='message'
               AND json_extract(input.body,'$.provenance.author')='user'
               AND coalesce(json_extract(latest_run.body,'$.notice'),0)=0
               AND EXISTS(SELECT 1 FROM json_each(input.body,'$.content') part
                          WHERE json_extract(part.value,'$.type')='text')
             ORDER BY latest.sequence DESC LIMIT 1)
         ORDER BY h.sequence DESC LIMIT 1"
    ).await.map_err(storage_error)?;
    let row = match query.query_row([session.to_string()]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let body: Vec<u8> = row.get(0).map_err(storage_error)?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let index = record
        .metadata
        .input
        .as_ref()
        .and_then(|input| input.capability_part);
    Ok(record
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
        .find_map(|block| match &block.kind {
            BlockKind::Text { text } if Some(block.index) == index => Some(text.clone()),
            _ => None,
        }))
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
    mut effects: Vec<Effect>,
    presentation: Presentation,
    timeline: Option<Arc<crate::agent::observation::Timeline>>,
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
    // Goose associates request usage with the last non-error assistant message.
    for index in 0..effects.len() {
        let Effect::Usage(usage) = &effects[index] else {
            continue;
        };
        let usage = usage.clone();
        if let Some(message) = effects.iter_mut().rev().find_map(|effect| match effect {
            Effect::Conversation(ConversationEffect::AppendMessage(message))
                if message.role == rmcp::model::Role::Assistant
                    && message.error_kind().is_none() =>
            {
                Some(message)
            }
            _ => None,
        }) {
            message.metadata.usage =
                Some(Box::new(MessageUsage::from_provider_usage(&usage, false)));
        }
    }
    let usage = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Usage(usage) => Some(usage.clone()),
            _ => None,
        })
        .next_back();
    let last=effects.iter().rposition(|effect|matches!(effect,Effect::Conversation(ConversationEffect::AppendMessage(message)) if message.role==rmcp::model::Role::Assistant && message.error_kind().is_none()));
    for (index, effect) in effects.into_iter().enumerate() {
        match effect {
            Effect::Usage(_) => {}
            Effect::Records(records) => {
                for mut record in records {
                    record.metadata.presentation = Some(presentation.clone());
                    history::append(database, session, record.turn, record, events).await?;
                }
            }
            Effect::Approval { .. } => {
                unreachable!("approval is resolved before submitting storage effects")
            }
            Effect::Conversation(ConversationEffect::AppendMessage(message)) => {
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
                    history::Message {
                        message,
                        usage: (last == Some(index)).then(|| usage.clone()).flatten(),
                        calls: calls.clone(),
                        presentation: presentation.clone(),
                        timeline: timeline.clone(),
                    },
                    events,
                )
                .await?;
            }
            Effect::Compaction {
                summary,
                usage,
                files,
            } => save_summary(database, session, turn, *summary, usage, files, events).await?,
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
        history::Summary {
            message: summary,
            usage,
            boundary: boundary.id.clone(),
            start: current
                .records
                .first()
                .map_or(boundary.created, |record| record.created),
            end: boundary.created,
            files,
        },
        events,
    )
    .await
}
