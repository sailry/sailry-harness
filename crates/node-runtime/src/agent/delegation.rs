//! An ADK delegation tool admits child work into the existing Node supervisor.
use super::*;
use adk_core::{AdkError, Tool, ToolConfirmationRequest, ToolContext};
use async_trait::async_trait;
use sailry_protocol::conversation::Input;
use serde_json::{Value, json};
mod control;
mod observation;
mod scheduler;
pub(super) mod selected;
pub(in crate::agent) use control::Control;

fn selected_call(turn: TurnId) -> String {
    format!("sailry-selected-{turn}")
}

pub(super) fn selection(
    invocation: &Invocation,
    name: String,
) -> Result<adk_core::BeforeModelCallback, Fault> {
    use adk_core::{BeforeModelResult, Content, LlmResponse, Part};
    use std::sync::atomic::{AtomicBool, Ordering};
    let arguments = selected::arguments(invocation)?;
    let pending = AtomicBool::new(invocation.message.selected_role().is_some());
    let call = selected_call(invocation.turn.id);
    Ok(Box::new(move |_, request| {
        let response = if pending.swap(false, Ordering::AcqRel) {
            let mut content = Content::new("model");
            content.parts.push(Part::FunctionCall {
                id: Some(call.clone()),
                name: name.clone(),
                args: arguments.clone(),
                thought_signature: None,
            });
            BeforeModelResult::Skip(LlmResponse {
                content: Some(content),
                ..Default::default()
            })
        } else {
            BeforeModelResult::Continue(request)
        };
        Box::pin(async move { Ok(response) })
    }))
}

pub(super) struct Delegate {
    ingress: Arc<Ingress>,
    tasks: Tasks,
    turn: TurnId,
    session: String,
    parent: sailry_protocol::SessionId,
    stop: CancellationToken,
    name: String,
    description: String,
    parameters: Value,
    selected: Option<Input>,
}

impl Delegate {
    pub fn new(
        ingress: &Arc<Ingress>,
        tasks: &Tasks,
        invocation: &Invocation,
        stop: &CancellationToken,
        name: String,
        description: String,
        parameters: Value,
    ) -> Self {
        Self {
            ingress: ingress.clone(),
            tasks: tasks.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session.to_string(),
            parent: invocation.turn.session,
            stop: stop.clone(),
            name,
            parameters,
            description,
            selected: invocation.message.selected_role().map(|_| {
                let mut message = invocation.message.clone();
                message.references.retain(|reference| {
                    !matches!(
                        reference.target,
                        sailry_protocol::conversation::reference::Target::Agent(_)
                    )
                });
                if message.is_empty() {
                    message.text = "Complete the request using the selected context".into();
                }
                message
            }),
        }
    }
}

struct Active {
    ingress: Arc<Ingress>,
    turn: TurnId,
}

struct Child {
    ingress: Arc<Ingress>,
    parent: sailry_protocol::SessionId,
    child: sailry_protocol::SessionId,
    turn: TurnId,
}

impl Drop for Child {
    fn drop(&mut self) {
        self.ingress
            .agents
            .finish_child(self.parent, self.child, self.turn);
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.ingress.agents.finish_operation(self.turn);
    }
}

#[async_trait]
impl Tool for Delegate {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn is_agent_delegation(&self) -> bool {
        true
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(self.parameters.clone())
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        self.execute_prepared(context, arguments.clone(), arguments)
            .await
    }
}

impl Delegate {
    pub(super) async fn execute_prepared(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
        original: Value,
    ) -> adk_core::Result<Value> {
        let deadline = tokio::time::Instant::now() + observation::window(&self.ingress.agents);
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(AdkError::tool("delegation is no longer active"));
        }
        let parent_stop = self
            .ingress
            .agents
            .begin_operation(self.turn)
            .ok_or_else(|| AdkError::tool("parent execution has ended"))?;
        let background = arguments
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // The session owns admitted work; an attached observer still cancels it on drop.
        let stop = self.ingress.agents.stopped.child_token();
        let active = Active {
            ingress: self.ingress.clone(),
            turn: self.turn,
        };
        let mut observer = observation::Observer::new(stop.clone(), background);
        let ingress = self.ingress.clone();
        let tasks = self.tasks.clone();
        let turn = self.turn;
        let parent = self.parent;
        let message = (context.function_call_id() == selected_call(turn))
            .then(|| self.selected.clone())
            .flatten();
        let request = ToolConfirmationRequest {
            tool_name: self.name().into(),
            function_call_id: Some(context.function_call_id().into()),
            args: original,
        };
        let (admitted, admission) = tokio::sync::oneshot::channel();
        let task_stop = stop.clone();
        let (deferred, mut defer) = tokio::sync::oneshot::channel();
        let result = self.tasks.spawn_child(&self.ingress.agents.stopped, async move {
            let stop = task_stop;
            let _cancel = stop.clone().drop_guard();
            let mut active = Some(active);
            let mut observing = true;
            // A boundary handoff cancels one Runner, never the parent-owned operation.
            let mut token = stop.child_token();
            let mut invocation = ingress.delegate(turn, request, arguments, message, token.clone()).await?;
            ingress.agents.register_child(parent, invocation.turn.session, invocation.turn.id, stop.clone());
            let _child = Child { ingress: ingress.clone(), parent, child: invocation.turn.session, turn: invocation.turn.id };
            if parent_stop.is_cancelled() { stop.cancel(); }
            if background { active.take(); }
            let _ = admitted.send((invocation.turn.session, invocation.turn.id));
            loop {
                let session = invocation.turn.session;
                let turn = invocation.turn.id;
                let execution = scheduler::run(&ingress, &tasks, invocation, token, stop.clone());
                tokio::pin!(execution);
                let mut result = if active.is_some() && observing {
                    tokio::select! {
                        result = &mut execution => result,
                        detached = &mut defer => {
                            observing = false;
                            if detached.is_ok() { active.take(); }
                            execution.await
                        }
                    }
                } else { execution.await };
                let next_token = stop.child_token();
                let next = if result.is_err() || stop.is_cancelled() || ingress.agents.stopped.is_cancelled() {
                    ingress.retain_guidance(turn).await?;
                    None
                } else {
                    match ingress.claim_guidance(turn, next_token.clone()).await {
                        Ok(next) => next,
                        Err(error) => {
                            ingress.retain_guidance(turn).await?;
                            result = Err(error);
                            None
                        }
                    }
                };
                if let Some(next) = next {
                    invocation = next;
                    token = next_token;
                    continue;
                }
                return Ok::<_, Fault>(match result {
                    Ok((status, response)) => json!({"session": session, "turn": turn, "status": status, "response": response}),
                    Err(error) => json!({"isError": true, "session": session, "turn": turn, "error": error}),
                });
            }
        }).await.map_err(|_| AdkError::tool("child supervisor is unavailable"))?;
        let (session, turn) = match admission.await {
            Ok(ids) => ids,
            Err(_) => {
                return match result.await {
                    Ok(Err(error)) => Ok(json!({"isError":true,"error":error})),
                    _ => Err(AdkError::tool("child admission outcome is unknown")),
                };
            }
        };
        if background {
            return Ok(
                json!({"admitted":true,"pending":true,"session":session,"turn":turn,"status":"queued"}),
            );
        }
        let observed = observation::wait(
            result,
            &self.stop,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await
        .map_err(|error| AdkError::tool(error.message))?;
        let Some(result) = observed else {
            // Releasing observation is not cancellation or a new execution admission.
            observer.detach();
            let state = self
                .ingress
                .child_state_at(self.turn, session, Some(turn))
                .await;
            let _ = deferred.send(());
            return match state {
                Ok(state)
                    if matches!(
                        state.run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    ) =>
                {
                    Ok(observation::pending(&state.run))
                }
                Ok(state) => {
                    self.ingress
                        .collect_child(self.turn, session, turn)
                        .await
                        .map_err(|error| AdkError::tool(error.message))?;
                    Ok(
                        json!({"session":session,"turn":turn,"status":state.run.status,"response":state.response,"error":state.run.error}),
                    )
                }
                Err(error) => Ok(
                    json!({"admitted":true,"pending":true,"session":session,"turn":turn,"state_unavailable":true,"error":error}),
                ),
            };
        };
        match result {
            Ok(Ok(value)) => {
                if let (Some(session), Some(turn)) = (value.get("session"), value.get("turn")) {
                    let session = serde_json::from_value(session.clone())
                        .map_err(|_| AdkError::tool("child result session is invalid"))?;
                    let turn = serde_json::from_value(turn.clone())
                        .map_err(|_| AdkError::tool("child result turn is invalid"))?;
                    self.ingress
                        .collect_child(self.turn, session, turn)
                        .await
                        .map_err(|fault| AdkError::tool(fault.message))?;
                }
                Ok(value)
            }
            Ok(Err(error)) => Ok(json!({"isError":true, "error": error})),
            Err(_) => Err(AdkError::tool("child task outcome is unknown")),
        }
    }
}
