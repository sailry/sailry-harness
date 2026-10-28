//! The parent controls children through durable Node admission.
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
    resume: Delegate,
}

impl Control {
    fn is_read_only(&self) -> bool {
        false
    }
    pub(in crate::agent) fn new(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        package: &plugin::Info,
        stop: &CancellationToken,
        operation: Operation,
        resume: Delegate,
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
            resume,
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

impl Control {
    fn name(&self) -> &str {
        self.resume.name()
    }
    fn description(&self) -> &str {
        self.resume.description()
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
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
        {
            return Err(Fault::invalid("child control is no longer active"));
        }
        let loaded = async {
            let args: Arguments = serde_json::from_value(arguments.clone())
                .map_err(|_| Fault::invalid("invalid child control arguments"))?;
            if self.operation == Operation::GuideAgent {
                if args.turn.is_some() {
                    return Err(Fault::invalid("guidance does not accept an execution turn"));
                }
                if args
                    .message
                    .as_ref()
                    .is_none_or(|message| message.trim().is_empty())
                {
                    return Err(Fault::invalid("child guidance message is required"));
                }
            } else if args.message.is_some() {
                return Err(Fault::invalid("waiting does not accept a message"));
            }
            let state = self.ingress.child_state(self.turn, args.session).await?;
            Ok((args, state))
        }
        .await;
        let (args, state) = match loaded {
            Ok(loaded) => loaded,
            Err(error) => {
                return crate::agent::catalog::json(self.failure(context.as_ref(), error));
            }
        };
        if self.operation == Operation::GuideAgent
            && !matches!(
                state.run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
        {
            let mut output = self
                .resume
                .execute_prepared(
                    context,
                    json!({"session":args.session,"task":args.message.unwrap()}),
                    arguments,
                )
                .await?;
            let mut value = output
                .structured_content
                .take()
                .ok_or_else(|| Fault::invalid("child result is unavailable"))?;
            if value.get("session") == Some(&json!(args.session)) {
                value["recipient"] = json!(state.run.turn);
            }
            let mut result = crate::agent::catalog::json(value)?;
            result.meta = output.meta;
            return Ok(result);
        }
        let work = async {
            if self.operation == Operation::GuideAgent {
                let message = args.message.unwrap();
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
            let deadline = tokio::time::Instant::now() + observation::window(&self.ingress.agents);
            loop {
                let notified = self.ingress.agents.wake.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
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
                // Settle the complete tool batch before the next model boundary
                // inserts queued guidance into this invocation.
                if self.ingress.pending_guidance(self.turn).await? {
                    return Ok(
                        json!({"session":state.run.session,"turn":state.run.turn,"status":state.run.status,"guidance_pending":true}),
                    );
                }
                tokio::select! {
                    _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "child wait was cancelled")),
                    _ = tokio::time::sleep_until(deadline) => return Ok(observation::pending(&state.run)),
                    _ = notified => {},
                }
            }
        };
        crate::agent::catalog::json(match work.await {
            Ok(output) => output,
            Err(error) => self.failure(context.as_ref(), error),
        })
    }
}

impl Control {
    fn failure(&self, context: &CallContext, error: Fault) -> Value {
        if let Some(timeline) = self.ingress.agents.timeline(self.turn) {
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                self.name(),
                Some(error.code),
            );
        }
        json!({"isError":true,"error":error})
    }
}

crate::agent::catalog::provider!(Control, crate::agent::catalog::result);
