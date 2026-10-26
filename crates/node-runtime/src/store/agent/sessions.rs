use super::*;
use adk_session::{
    AppendEventRequest, CreateRequest, DeleteRequest, Events, ListRequest, SessionService, State,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn history_query() -> String {
    format!("SELECT e.body FROM conversation_events h
    JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND {} ORDER BY h.sequence DESC LIMIT ?2", history::VISIBLE_EVENT)
}
pub(crate) const INPUT_TURN: &str = "sailry.context.input_turn";
pub(crate) const INPUT_SOURCE: &str = "sailry.context.input_source";
pub(crate) const INPUT_PART: &str = "sailry.context.input_part";

pub(crate) struct Sessions {
    ingress: Arc<Ingress>,
    turn: TurnId,
    input_turn: TurnId,
    input_source: &'static str,
    references: Vec<reference::Reference>,
    notices: Vec<sailry_protocol::conversation::background::Notice>,
    notice_input: Option<String>,
    groupings: std::collections::BTreeMap<String, sailry_protocol::tool::Grouping>,
    presentations: std::collections::BTreeMap<String, sailry_protocol::tool::Presentation>,
    displays: std::collections::BTreeMap<String, sailry_protocol::tool::Display>,
    bindings: std::collections::BTreeMap<String, ApprovalBinding>,
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
            notices: Vec::new(),
            notice_input: None,
            presentations: Default::default(),
            groupings: Default::default(),
            displays: Default::default(),
            bindings: Default::default(),
            connections: Default::default(),
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
        groupings: std::collections::BTreeMap<String, sailry_protocol::tool::Grouping>,
    ) -> Self {
        self.groupings = groupings;
        self
    }

    pub(crate) fn with_presentations(
        mut self,
        presentations: std::collections::BTreeMap<String, sailry_protocol::tool::Presentation>,
    ) -> Self {
        self.presentations = presentations;
        self
    }

    pub(crate) fn with_displays(
        mut self,
        displays: std::collections::BTreeMap<String, sailry_protocol::tool::Display>,
    ) -> Self {
        self.displays = displays;
        self
    }

    pub(crate) fn with_connection_context(
        mut self,
        bindings: std::collections::BTreeMap<String, ApprovalBinding>,
        connections: Vec<sailry_protocol::connection::Resource>,
    ) -> Self {
        self.bindings = bindings;
        self.connections = connections;
        self
    }

    async fn current_displays(
        &self,
        event: &AdkEvent,
    ) -> adk_core::Result<std::collections::BTreeMap<String, sailry_protocol::tool::Display>> {
        use sailry_protocol::{connection::Resource, tool::Operation};

        let mut displays = self.displays.clone();
        let mut pending = Vec::new();
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            let adk_core::Part::FunctionCall { name, args, .. } = part else {
                continue;
            };
            let Some(field) = displays
                .get(name)
                .and_then(|display| display.input.as_ref())
                .and_then(|input| input.summary.as_ref())
                .filter(|field| {
                    field.format == Some(sailry_protocol::tool::InputFormat::Connection)
                })
            else {
                continue;
            };
            let Some(id) = field.read(args) else {
                continue;
            };
            if field.names.contains_key(id) {
                continue;
            }
            let Some(binding) = self.bindings.get(name) else {
                continue;
            };
            let target = match binding.operation {
                Some(Operation::RunSsh | Operation::TransferSsh) => {
                    id.parse().ok().map(Resource::Ssh)
                }
                Some(
                    Operation::BrowseDatabase
                    | Operation::QueryDatabase
                    | Operation::ExecuteDatabase,
                ) => id.parse().ok().map(Resource::Database),
                _ => None,
            };
            if let Some(target) = target {
                pending.push((name.clone(), id.to_owned(), binding.owner.clone(), target));
            }
        }
        if pending.is_empty() {
            return Ok(displays);
        }
        let names = self
            .ingress
            .connection_names(
                self.turn,
                pending
                    .iter()
                    .map(|(_, _, package, target)| (package.clone(), *target))
                    .collect(),
                self.connections.clone(),
            )
            .await
            .map_err(adk_error)?;
        for ((tool, id, _, _), name) in pending.into_iter().zip(names) {
            let Some(name) = name else {
                continue;
            };
            if let Some(field) = displays
                .get_mut(&tool)
                .and_then(|display| display.input.as_mut())
                .and_then(|input| input.summary.as_mut())
            {
                field.names.insert(id, name);
            }
        }
        Ok(displays)
    }
}

#[async_trait]
impl SessionService for Sessions {
    async fn create(&self, _: CreateRequest) -> adk_core::Result<Box<dyn AdkSession>> {
        Err(AdkError::session(
            "create product sessions through Node commands",
        ))
    }
    async fn delete(&self, _: DeleteRequest) -> adk_core::Result<()> {
        Err(AdkError::session(
            "delete product sessions through Node commands",
        ))
    }
    async fn list(&self, _: ListRequest) -> adk_core::Result<Vec<Box<dyn AdkSession>>> {
        Err(AdkError::session(
            "list product sessions through Node commands",
        ))
    }
    async fn get(&self, request: GetRequest) -> adk_core::Result<Box<dyn AdkSession>> {
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(Operation::Get {
                turn: self.turn,
                request,
                reply,
            })))
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?;
        response
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?
    }
    async fn append_event(&self, session_id: &str, mut event: AdkEvent) -> adk_core::Result<()> {
        let timeline = self.ingress.agents.timeline(self.turn);
        let failures = super::presentation::capture_failures(&mut event, timeline.as_deref())?;
        let branch = event.branch.clone();
        let displays = self.current_displays(&event).await?;
        let mut aliases = Vec::new();
        if let Some(home) = self.ingress.environment.home() {
            aliases.push(sailry_protocol::tool::Alias {
                path: home.to_string_lossy().into_owned(),
                kind: sailry_protocol::tool::AliasKind::Home,
            });
        }
        let commands: Vec<_> = event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
            .filter_map(|part| {
                let adk_core::Part::FunctionCall { name, args, .. } = part else {
                    return None;
                };
                let field = displays.get(name)?.input.as_ref()?.summary.as_ref()?;
                (field.format == Some(sailry_protocol::tool::InputFormat::Command))
                    .then(|| field.read(args))
                    .flatten()
            })
            .collect();
        if let Some(python) = self.ingress.plugins.display_python(&commands).await {
            aliases.push(sailry_protocol::tool::Alias {
                path: python.to_string_lossy().into_owned(),
                kind: sailry_protocol::tool::AliasKind::Python,
            });
        }
        for access in self.ingress.agents.python(self.turn) {
            let alias = sailry_protocol::tool::Alias {
                path: access.ready.python.to_string_lossy().into_owned(),
                kind: sailry_protocol::tool::AliasKind::Python,
            };
            if !aliases.contains(&alias) && aliases.len() < 32 {
                aliases.push(alias);
            }
        }
        super::presentation::stamp(
            &mut event,
            &self.presentations,
            &self.groupings,
            &displays,
            &aliases,
        )?;
        if event.author == "user" {
            event
                .provider_metadata
                .insert(INPUT_TURN.into(), self.input_turn.to_string());
            event
                .provider_metadata
                .insert(INPUT_SOURCE.into(), self.input_source.into());
            let notice = self.notice_input.as_ref().is_some_and(|expected| matches!(event.content().and_then(|content| content.parts.first()), Some(adk_core::Part::Text {text}) if text == expected));
            if notice {
                event.provider_metadata.insert(
                    super::background::INPUT_IDS.into(),
                    serde_json::to_string(
                        &self
                            .notices
                            .iter()
                            .map(|notice| &notice.id)
                            .collect::<Vec<_>>(),
                    )
                    .map_err(|error| AdkError::session(error.to_string()))?,
                );
            }
            let index = usize::from(!self.references.is_empty()) + usize::from(notice);
            if event.content().is_some_and(|content| {
                matches!(content.parts.get(index), Some(adk_core::Part::Text { .. }))
            }) {
                event
                    .provider_metadata
                    .insert(INPUT_PART.into(), index.to_string());
            }
        }
        if event.author == "user" && !self.references.is_empty() {
            event.provider_metadata.insert(
                super::references::METADATA.into(),
                serde_json::to_string(&self.references)
                    .map_err(|error| AdkError::session(error.to_string()))?,
            );
        }
        let session = session_id.parse().map_err(AdkError::session)?;
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit(Job::Agent(Box::new(Operation::Append {
                session,
                turn: if event.author == "user" {
                    self.input_turn
                } else {
                    self.turn
                },
                event,
                reply,
            })))
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?;
        let result = response
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?;
        if result.is_ok()
            && let Some(timeline) = timeline
        {
            for failure in failures {
                timeline.forget_failure(&branch, &failure.id, &failure.name);
            }
        }
        result
    }
    async fn append_event_for_identity(&self, request: AppendEventRequest) -> adk_core::Result<()> {
        identity(
            request.identity.app_name.as_ref(),
            request.identity.user_id.as_ref(),
        )?;
        self.append_event(request.identity.session_id.as_ref(), request.event)
            .await
    }
}

fn identity(app: &str, user: &str) -> adk_core::Result<()> {
    if app == APP && user == USER {
        Ok(())
    } else {
        Err(AdkError::session(
            "session identity does not belong to this Node",
        ))
    }
}

pub(super) async fn get(
    db: &Connection,
    turn: TurnId,
    request: GetRequest,
) -> adk_core::Result<Box<dyn AdkSession>> {
    identity(&request.app_name, &request.user_id)?;
    let session: SessionId = request.session_id.parse().map_err(AdkError::session)?;
    check_session(db, session).await.map_err(adk_error)?;
    let owns: bool = (async {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM turns WHERE id=?1 AND session=?2)")
            .await?;
        let row = statement
            .query_row(params![turn.to_string(), session.to_string()])
            .await?;
        let mapped: turso::Result<_> = row.get(0);
        mapped
    })
    .await
    .map_err(|error| adk_error(storage_error(error)))?;
    if !owns {
        return Err(AdkError::session(
            "session does not belong to this invocation",
        ));
    }
    // Bound the database read before decoding; requesting recent history must
    // not load a long conversation only to discard most of it afterwards.
    let limit = request
        .num_recent_events
        .map(|limit| {
            i64::try_from(limit.max(1)).map_err(|_| AdkError::session("history limit is invalid"))
        })
        .transpose()?
        .unwrap_or(-1);
    let summary = if request.num_recent_events.is_none() && request.after.is_none() {
        super::compaction::read(db, session).await?
    } else {
        None
    };
    let sql = if summary.is_some() {
        format!("SELECT e.body FROM conversation_events h
        JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence>?3
        AND json_type(e.body,'$.actions.compaction') IS NULL AND {} ORDER BY h.sequence DESC LIMIT ?2", history::VISIBLE_EVENT)
    } else {
        history_query()
    };
    let mut query = db
        .prepare(&sql)
        .await
        .map_err(|error| adk_error(storage_error(error)))?;
    let session_key = session.to_string();
    let boundary = summary.as_ref().map_or(0, |(sequence, _)| *sequence);
    let mut parameters = vec![
        turso::Value::Text(session_key),
        turso::Value::Integer(limit),
    ];
    if summary.is_some() {
        parameters.push(turso::Value::Integer(boundary));
    }
    let mut events = {
        let mut rows = query
            .query(parameters)
            .await
            .map_err(|error| adk_error(storage_error(error)))?;
        let mut values = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| adk_error(storage_error(error)))?
        {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|body| {
        serde_json::from_slice::<AdkEvent>(&body.map_err(|error| adk_error(storage_error(error)))?)
            .map_err(|error| adk_error(storage_error(error)))
    })
    .collect::<adk_core::Result<Vec<_>>>()?;
    events.reverse();
    if let Some((_, summary)) = summary {
        events.insert(0, summary);
    }
    let updated = events
        .last()
        .map_or(DateTime::<Utc>::UNIX_EPOCH, |event| event.timestamp);
    let state = history::state(db, session).await.map_err(adk_error)?;
    if request.num_recent_events == Some(0) {
        events.clear();
    }
    if let Some(after) = request.after {
        events.retain(|event| event.timestamp >= after);
    }
    Ok(Box::new(Session {
        id: session.to_string(),
        state,
        events,
        updated,
    }))
}

struct Session {
    id: String,
    state: HashMap<String, Value>,
    events: Vec<AdkEvent>,
    updated: DateTime<Utc>,
}
impl AdkSession for Session {
    fn id(&self) -> &str {
        &self.id
    }
    fn app_name(&self) -> &str {
        APP
    }
    fn user_id(&self) -> &str {
        USER
    }
    fn state(&self) -> &dyn State {
        self
    }
    fn events(&self) -> &dyn Events {
        self
    }
    fn last_update_time(&self) -> DateTime<Utc> {
        self.updated
    }
}
impl State for Session {
    fn get(&self, key: &str) -> Option<Value> {
        self.state.get(key).cloned()
    }
    fn set(&mut self, key: String, value: Value) {
        self.state.insert(key, value);
    }
    fn all(&self) -> HashMap<String, Value> {
        self.state.clone()
    }
}
impl Events for Session {
    fn all(&self) -> Vec<AdkEvent> {
        self.events.clone()
    }
    fn len(&self) -> usize {
        self.events.len()
    }
    fn at(&self, index: usize) -> Option<&AdkEvent> {
        self.events.get(index)
    }
}
