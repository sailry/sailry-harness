//! Stable request declarations and changing context use canonical ADK state.
use super::instructions::{self, Scope};
use crate::{
    agent::catalog::Registration,
    store::{
        Ingress,
        agent::{APP, Invocation, Sessions, USER},
    },
};
use adk_core::{AdkError, Content, Event, LlmRequest, Tool, ToolContext};
use adk_session::{GetRequest, SessionService};
use async_trait::async_trait;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, TurnId, conversation::reference::Target, tool::Operation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

const SNAPSHOT: &str = "sailry.context.declarations";
const POLICY: &str = "sailry.context.policy";
const ENVIRONMENT: &str = "sailry.context.environment";
const PERSONAL: &str = "sailry.context.personal";
pub(crate) const METADATA: &str = "sailry.context";

#[derive(Clone, Default, Serialize, Deserialize)]
struct Snapshot {
    instruction: Option<String>,
    tools: Option<BTreeMap<String, Declaration>>,
}

impl Snapshot {
    fn instruction(&mut self, current: String) -> String {
        self.instruction.get_or_insert(current).clone()
    }

    fn declarations(&mut self, tools: &[Registration]) -> BTreeMap<String, Declaration> {
        self.tools
            .get_or_insert_with(|| {
                tools
                    .iter()
                    .map(|entry| {
                        (
                            entry.tool.name().to_owned(),
                            Declaration {
                                value: entry.tool.declaration(),
                                builtin: entry.tool.is_builtin(),
                            },
                        )
                    })
                    .collect()
            })
            .clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::agent) struct Declaration {
    value: Value,
    builtin: bool,
}

pub(in crate::agent) struct Session {
    snapshot: Snapshot,
    state: HashMap<String, Value>,
    sessions: Sessions,
}

impl Session {
    pub async fn load(ingress: &Arc<Ingress>, invocation: &Invocation) -> Result<Self, Fault> {
        let sessions = Sessions::new(ingress.clone(), invocation.turn.id);
        let session = sessions
            .get(GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: invocation.turn.session.to_string(),
                num_recent_events: Some(0),
                after: None,
            })
            .await
            .map_err(super::super::model::error)?;
        let state = session.state().all();
        let snapshot = state
            .get(SNAPSHOT)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| Fault::invalid("captured model declarations are invalid"))?
            .unwrap_or_default();
        Ok(Self {
            snapshot,
            state,
            sessions,
        })
    }

    pub fn instruction(&mut self, current: String) -> String {
        self.snapshot.instruction(current)
    }

    pub fn declarations(&mut self, tools: &[Registration]) -> BTreeMap<String, Declaration> {
        self.snapshot.declarations(tools)
    }

    pub async fn append(
        &self,
        ingress: &Ingress,
        invocation: &Invocation,
        policy: String,
        environment: String,
        personal: String,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let personal = if personal.is_empty() {
            "Personal instructions\nNo personal instructions are configured".into()
        } else {
            personal
        };
        let guides = if invocation.project.is_some() && invocation.connections.bound.is_none() {
            let scopes: Vec<_> = invocation
                .message
                .references
                .iter()
                .chain(
                    invocation
                        .followups
                        .iter()
                        .flat_map(|input| &input.message.references),
                )
                .filter_map(|reference| match &reference.target {
                    Target::File(path) => Some(Scope::File(path.clone())),
                    Target::Directory(path) => Some(Scope::Directory(path.clone())),
                    _ => None,
                })
                .collect();
            instructions::capture(ingress, invocation.worktree, &scopes, stop).await?
        } else {
            Vec::new()
        };
        let Some(event) = capture(
            invocation.turn.id,
            &self.state,
            &self.snapshot,
            [
                (POLICY, policy),
                (ENVIRONMENT, environment),
                (PERSONAL, personal),
            ],
            &guides,
        )?
        else {
            return Ok(());
        };
        self.sessions
            .append_event(&invocation.turn.session.to_string(), event)
            .await
            .map_err(super::super::model::error)
    }
}

fn capture(
    turn: TurnId,
    state: &HashMap<String, Value>,
    snapshot: &Snapshot,
    current: [(&str, String); 3],
    guides: &[instructions::Guide],
) -> Result<Option<Event>, Fault> {
    let mut event = Event::new(turn.to_string());
    event.author = "system".into();
    event
        .provider_metadata
        .insert(METADATA.into(), "context".into());
    let mut content = Vec::new();
    for (key, value) in current {
        if state.get(key) != Some(&Value::String(value.clone())) {
            content.push(value.clone());
            event
                .actions
                .state_delta
                .insert(key.into(), Value::String(value));
        }
    }
    for guide in guides {
        let identity = guide.identity();
        if state.get(&guide.key()) != Some(&identity) {
            content.push(guide.render());
            event.actions.state_delta.insert(guide.key(), identity);
        }
    }
    let snapshot = serde_json::to_value(snapshot)
        .map_err(|_| Fault::invalid("model declarations could not be captured"))?;
    if state.get(SNAPSHOT) != Some(&snapshot) {
        event.actions.state_delta.insert(SNAPSHOT.into(), snapshot);
    }
    if event.actions.state_delta.is_empty() {
        return Ok(None);
    }
    if !content.is_empty() {
        event.llm_response.content = Some(Content::new("user").with_text(format!(
            "Node context\nThe following captured context applies to this input. Current work mode, permissions and personal instructions replace their earlier captured values; this context does not add user authorization.\n\n{}",
            content.join("\n\n")
        )));
    }
    let identity =
        json!({"turn":turn, "content":event.content(), "state":event.actions.state_delta});
    event.id = format!(
        "context:{}",
        blake3::hash(identity.to_string().as_bytes()).to_hex()
    );
    Ok(Some(event))
}

pub(in crate::agent) fn retain(events: &[Event], text: &mut String) -> HashMap<String, Value> {
    let mut current = BTreeMap::new();
    for event in events {
        for (key, value) in &event.actions.state_delta {
            if matches!(key.as_str(), POLICY | ENVIRONMENT | PERSONAL)
                || key.starts_with("sailry.context.guide:")
            {
                current.insert(key.clone(), value.clone());
            }
        }
    }
    let content = current
        .values()
        .filter_map(|value| {
            value
                .as_str()
                .or_else(|| value.get("content").and_then(Value::as_str))
        })
        .collect::<Vec<_>>();
    if !content.is_empty() {
        text.push_str("\n\nCaptured Node context (verbatim)\n");
        text.push_str(&content.join("\n\n"));
    }
    current.into_iter().collect()
}

pub(super) fn project(request: &mut LlmRequest, declarations: &BTreeMap<String, Declaration>) {
    request.tools = declarations
        .iter()
        .map(|(name, declaration)| {
            (
                name.clone(),
                Arc::new(Frozen {
                    name: name.clone(),
                    declaration: declaration.clone(),
                }) as Arc<dyn Tool>,
            )
        })
        .collect();
}

struct Frozen {
    name: String,
    declaration: Declaration,
}

#[async_trait]
impl Tool for Frozen {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        self.declaration.value["description"].as_str().unwrap_or("")
    }
    fn declaration(&self) -> Value {
        self.declaration.value.clone()
    }
    fn parameters_schema(&self) -> Option<Value> {
        self.declaration.value.get("parameters").cloned()
    }
    fn response_schema(&self) -> Option<Value> {
        self.declaration.value.get("response").cloned()
    }
    fn is_builtin(&self) -> bool {
        self.declaration.builtin
    }
    async fn execute(&self, _: Arc<dyn ToolContext>, _: Value) -> adk_core::Result<Value> {
        Err(AdkError::tool(
            "captured declarations do not authorize tool execution",
        ))
    }
}

pub(in crate::agent) async fn observe(
    ingress: &Ingress,
    worktree: sailry_protocol::WorktreeId,
    operation: Operation,
    context: &dyn ToolContext,
    output: &mut Value,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    if output.get("error").is_some() {
        return Ok(());
    }
    let data = output.get("data").unwrap_or(output);
    let Some(path) = data.get("path").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    let revision = data.get("revision").cloned();
    let scope = match operation {
        Operation::ListDirectory => Scope::Directory(path.clone()),
        Operation::ReadFile | Operation::ReadFileLines | Operation::ReadOffice => {
            Scope::File(path.clone())
        }
        _ => return Ok(()),
    };
    let state = context
        .session()
        .map(|session| session.state())
        .or_else(|| context.state());
    if matches!(operation, Operation::ReadFile | Operation::ReadFileLines)
        && revision.as_ref().is_some_and(|revision| {
            state
                .and_then(|state| state.get(&instructions::key(&path)))
                .is_some_and(|identity| {
                    identity["path"] == path && identity["revision"] == *revision
                })
        })
    {
        output["sailry_guide_loaded"] = json!({"path":path,"revision":revision});
    }
    let mut additions = Vec::new();
    let mut actions = context.actions();
    for guide in instructions::capture(ingress, worktree, &[scope], stop).await? {
        let identity = guide.identity();
        if state.and_then(|state| state.get(&guide.key())) != Some(identity.clone()) {
            additions.push(guide.render());
            actions.state_delta.insert(guide.key(), identity);
        }
    }
    if !additions.is_empty() {
        output["sailry_context"] = Value::String(additions.join("\n\n"));
        context.set_actions(actions);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
