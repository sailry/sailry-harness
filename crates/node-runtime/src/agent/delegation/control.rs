//! The main ADK agent controls children through durable Node admission.
use super::*;
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, Request, SessionId, plugin, tool::Operation};

pub(in crate::agent) struct Control {
    ingress: Arc<Ingress>,
    transport: Local,
    turn: TurnId,
    session: String,
    context: plugin::Context,
    stop: CancellationToken,
    operation: Operation,
    name: String,
    description: String,
}

impl Control {
    pub(in crate::agent) fn new(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        package: &plugin::Info,
        stop: &CancellationToken,
        operation: Operation,
        name: String,
        description: String,
    ) -> Self {
        Self {
            ingress: ingress.clone(),
            transport: Local::new(ingress.node, invocation.caller, ingress.clone()),
            turn: invocation.turn.id,
            session: invocation.turn.session.to_string(),
            context: plugin::Context {
                invocation: None,
                turn: Some(invocation.turn.id),
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(invocation.worktree),
                session: Some(invocation.turn.session),
            },
            stop: stop.clone(),
            operation,
            name,
            description,
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    session: SessionId,
    message: Option<String>,
    turn: Option<TurnId>,
}

#[async_trait]
impl Tool for Control {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(if self.operation == Operation::GuideAgent {
            json!({"type":"object","properties":{"session":{"type":"string"},"message":{"type":"string","minLength":1}},"required":["session","message"],"additionalProperties":false})
        } else {
            json!({"type":"object","properties":{"session":{"type":"string"},"turn":{"type":"string","description":"Exact execution from a completion notice; omit for the latest execution"}},"required":["session"],"additionalProperties":false})
        })
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(AdkError::tool("child control is no longer active"));
        }
        let work = async {
            let args: Arguments = serde_json::from_value(arguments)
                .map_err(|_| Fault::invalid("invalid child control arguments"))?;
            let state = self.ingress.child_state(self.turn, args.session).await?;
            if self.operation == Operation::GuideAgent {
                if args.turn.is_some() {
                    return Err(Fault::invalid("guidance does not accept an execution turn"));
                }
                let message = args
                    .message
                    .ok_or_else(|| Fault::invalid("child guidance message is required"))?;
                if state.run.status != sailry_protocol::conversation::Status::Running {
                    return Err(Fault::new(
                        ErrorCode::Conflict,
                        "guidance requires a running child",
                    ));
                }
                let request = Request::new(
                    self.ingress.node,
                    Command::GuideTurn {
                        recipient: state.run.turn,
                        expected_revision: state.run.revision,
                        message: message.into(),
                    },
                )
                .with_plugin(self.context.clone());
                let admission = self.transport.dispatch(request).await?;
                let output = admission.completion.await.map_err(|_| {
                    Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "child guidance outcome is unavailable",
                    )
                })??;
                let sailry_protocol::Output::QueuedTurn(turn) = output else {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "child guidance returned an unexpected outcome",
                    ));
                };
                return Ok(
                    json!({"session":turn.session,"turn":turn.id,"recipient":state.run.turn,"status":"queued"}),
                );
            }
            if args.message.is_some() {
                return Err(Fault::invalid("waiting does not accept a message"));
            }
            let deadline = tokio::time::Instant::now() + observation::window(&self.ingress.agents);
            loop {
                let state = self
                    .ingress
                    .child_state_at(self.turn, args.session, args.turn)
                    .await?;
                if state.paused && state.run.status == sailry_protocol::conversation::Status::Queued
                {
                    return Ok(
                        json!({"session":state.run.session,"turn":state.run.turn,"status":state.run.status,"paused":true}),
                    );
                }
                if !matches!(
                    state.run.status,
                    sailry_protocol::conversation::Status::Queued
                        | sailry_protocol::conversation::Status::Running
                        | sailry_protocol::conversation::Status::Stopping
                ) {
                    self.ingress
                        .collect_child(self.turn, args.session, state.run.turn)
                        .await?;
                    return Ok(
                        json!({"session":state.run.session,"turn":state.run.turn,"status":state.run.status,"response":state.response,"error":state.run.error}),
                    );
                }
                // Returning from this tool lets the complete batch settle before
                // the before-model callback hands off the main-session Runner.
                if self.ingress.pending_guidance(self.turn).await? {
                    return Ok(
                        json!({"session":state.run.session,"turn":state.run.turn,"status":state.run.status,"guidance_pending":true}),
                    );
                }
                tokio::select! {
                    _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "child wait was cancelled")),
                    _ = tokio::time::sleep_until(deadline) => return Ok(observation::pending(&state.run)),
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {},
                }
            }
        };
        Ok(match work.await {
            Ok(output) => output,
            Err(error) => json!({"isError":true,"error":error}),
        })
    }
}
