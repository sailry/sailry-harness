use super::*;
use adk_session::{
    AppendEventRequest, CreateRequest, DeleteRequest, Events, ListRequest, SessionService, State,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;

pub(super) const HISTORY_QUERY: &str = "SELECT e.body FROM conversation_events h
    JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence DESC LIMIT ?2";

pub(crate) struct Sessions {
    ingress: Arc<Ingress>,
    turn: TurnId,
    references: Vec<reference::Reference>,
    groupings: std::collections::BTreeMap<String, sailry_protocol::tool::Grouping>,
    presentations: std::collections::BTreeMap<String, sailry_protocol::tool::Presentation>,
    displays: std::collections::BTreeMap<String, sailry_protocol::tool::Display>,
}

impl Sessions {
    pub(crate) fn new(ingress: Arc<Ingress>, turn: TurnId) -> Self {
        Self {
            ingress,
            turn,
            references: Vec::new(),
            presentations: Default::default(),
            groupings: Default::default(),
            displays: Default::default(),
        }
    }

    pub(crate) fn with_references(mut self, references: Vec<reference::Reference>) -> Self {
        self.references = references;
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
            .sender
            .send(Job::Agent(Box::new(Operation::Get {
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
        super::presentation::stamp(
            &mut event,
            &self.presentations,
            &self.groupings,
            &self.displays,
        )?;
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
            .sender
            .send(Job::Agent(Box::new(Operation::Append {
                session,
                turn: self.turn,
                event,
                reply,
            })))
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?;
        response
            .await
            .map_err(|_| AdkError::session("Node storage is closed"))?
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

pub(super) fn get(
    db: &Connection,
    turn: TurnId,
    request: GetRequest,
) -> adk_core::Result<Box<dyn AdkSession>> {
    identity(&request.app_name, &request.user_id)?;
    let session: SessionId = request.session_id.parse().map_err(AdkError::session)?;
    check_session(db, session).map_err(adk_error)?;
    let owns: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM turns WHERE id=?1 AND session=?2)",
            params![turn.to_string(), session.to_string()],
            |row| row.get(0),
        )
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
        super::compaction::read(db, session)?
    } else {
        None
    };
    let context_query = "SELECT e.body FROM conversation_events h
        JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence>?3
        AND json_type(e.body,'$.actions.compaction') IS NULL ORDER BY h.sequence DESC LIMIT ?2";
    let mut query = db
        .prepare(if summary.is_some() {
            context_query
        } else {
            HISTORY_QUERY
        })
        .map_err(|error| adk_error(storage_error(error)))?;
    let session_key = session.to_string();
    let boundary = summary.as_ref().map_or(0, |(sequence, _)| *sequence);
    let mut parameters: Vec<&dyn rusqlite::ToSql> = vec![&session_key, &limit];
    if summary.is_some() {
        parameters.push(&boundary);
    }
    let mut events = query
        .query_map(parameters.as_slice(), |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| adk_error(storage_error(error)))?
        .map(|body| {
            serde_json::from_slice::<AdkEvent>(
                &body.map_err(|error| adk_error(storage_error(error)))?,
            )
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
    let state = history::state(db, session).map_err(adk_error)?;
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
