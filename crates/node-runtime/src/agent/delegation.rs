//! An ADK delegation tool admits child work into the existing Node supervisor.
use super::*;
use adk_core::{AdkError, Tool, ToolConfirmationRequest, ToolContext};
use async_trait::async_trait;
use sailry_protocol::conversation::Input;
use serde_json::{Value, json};

fn selected_call(turn: TurnId) -> String {
    format!("sailry-selected-{turn}")
}

pub(super) fn selection(
    invocation: &Invocation,
    name: String,
) -> Result<adk_core::BeforeModelCallback, Fault> {
    use adk_core::{BeforeModelResult, Content, LlmResponse, Part};
    use std::sync::atomic::{AtomicBool, Ordering};
    let selected = invocation
        .message
        .selected_role()
        .map(|selected| {
            invocation
                .turn
                .roles
                .profiles
                .iter()
                .find(|role| role.reference() == selected)
                .map(|role| role.key.clone())
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::InvalidRequest,
                        "selected role is absent from the frozen roster",
                    )
                })
        })
        .transpose()?;
    let pending = AtomicBool::new(selected.is_some());
    let call = selected_call(invocation.turn.id);
    let task = if invocation.message.text.trim().is_empty() {
        "Complete the request using the selected context".into()
    } else {
        invocation.message.text.clone()
    };
    Ok(Box::new(move |_, request| {
        let response = if pending.swap(false, Ordering::AcqRel) {
            let mut content = Content::new("model");
            content.parts.push(Part::FunctionCall {
                id: Some(call.clone()),
                name: name.clone(),
                args: json!({"role": selected, "task": task}),
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
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(AdkError::tool("delegation is no longer active"));
        }
        let stop = self
            .ingress
            .agents
            .begin_operation(self.turn)
            .ok_or_else(|| AdkError::tool("parent execution has ended"))?
            .child_token();
        let active = Active {
            ingress: self.ingress.clone(),
            turn: self.turn,
        };
        // ADK timeout/drop cancels the child, while the supervisor retains cleanup ownership.
        let _cancel = stop.clone().drop_guard();
        let ingress = self.ingress.clone();
        let tasks = self.tasks.clone();
        let turn = self.turn;
        let message = (context.function_call_id() == selected_call(turn))
            .then(|| self.selected.clone())
            .flatten();
        let request = ToolConfirmationRequest {
            tool_name: self.name().into(),
            function_call_id: Some(context.function_call_id().into()),
            args: original,
        };
        let result = self.tasks.spawn(async move {
            let _active = active;
            let invocation = ingress.delegate(turn, request, arguments, message, stop.clone()).await?;
            let session = invocation.turn.session;
            let turn = invocation.turn.id;
            let result = Box::pin(run(&ingress, &tasks, invocation, stop)).await;
            Ok::<_, Fault>(match result {
                Ok((status, response)) => json!({"session": session, "turn": turn, "status": status, "response": response}),
                Err(error) => json!({"isError": true, "session": session, "turn": turn, "error": error}),
            })
        }).map_err(|_| AdkError::tool("child task capacity is unavailable"))?;
        match result.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Ok(json!({"isError":true, "error": error})),
            Err(_) => Err(AdkError::tool("child task outcome is unknown")),
        }
    }
}
