//! Package tools share the bounded headless VM and the original Agent history.
use super::*;
use adk_core::{AdkError, Tool, ToolContext};
use async_trait::async_trait;
use sailry_link::Handler as _;
use sailry_protocol::{NodeId, plugin, tool};
use serde_json::{Value, json};
use std::sync::Mutex;
mod flow;
mod identity;
mod media;
mod policy;
mod turn;

pub(super) async fn bind(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Result<Contribution, Fault> {
    let mut declarations: Vec<_> = invocation
        .plugins
        .iter()
        .flat_map(|package| {
            package
                .extension
                .iter()
                .flat_map(|extension| &extension.tools)
                .filter(|tool| {
                    tool.valid()
                        && crate::plugins::conversation::selects(
                            invocation.turn.config.assistant.as_ref(),
                            &invocation.plugins,
                            package,
                            &tool.name,
                        )
                        && (invocation.connections.bound.is_none()
                            || invocation.turn.config.assistant.is_some()
                            || !tool.contexts.is_empty())
                        && tool.available_on(std::env::consts::OS)
                        && tool.available_in(invocation.project, invocation.connections.bound)
                        && tool.handler.as_ref().is_some_and(|handler| {
                            handler.operation.is_none_or(|operation| {
                                operations::available(ingress, invocation, operation)
                            }) && handler.flow.as_ref().is_none_or(|flow| {
                                flow.operations.iter().all(|operation| {
                                    operations::available(ingress, invocation, *operation)
                                })
                            })
                        })
                })
                .map(move |tool| (package, tool))
        })
        .collect();
    if declarations.is_empty() {
        return Ok(Contribution::default());
    }
    let mut contribution = Contribution::default();
    let mut turns = std::collections::BTreeMap::new();
    let mut parameters = std::collections::BTreeMap::new();
    for package in &invocation.plugins {
        if !package
            .extension
            .as_ref()
            .and_then(|extension| extension.host.as_ref())
            .is_some_and(|host| host.turn.is_some())
        {
            continue;
        }
        let selected: Vec<_> = declarations
            .iter()
            .filter(|(provider, _)| provider.summary.name == package.summary.name)
            .map(|(_, declaration)| declaration.name.clone())
            .collect();
        if selected.is_empty() {
            continue;
        }
        let initialized =
            turn::Turn::initialize(ingress, invocation, package, selected, stop).await?;
        declarations.retain(|(provider, declaration)| {
            provider.summary.name != package.summary.name
                || initialized.tools.contains(&declaration.name)
        });
        if let Some(callback) = initialized.owner.observer() {
            contribution.callbacks.push((10, callback));
        }
        contribution.instruction.push_str(&initialized.instruction);
        parameters.extend(
            initialized
                .parameters
                .into_iter()
                .map(|(name, schema)| ((package.summary.name.clone(), name), schema)),
        );
        turns.insert(package.summary.name.clone(), initialized.owner);
    }
    contribution.tools = declarations
        .into_iter()
        .map(|(package, declaration)| {
            let name = operations::alias(&package.summary.name, &declaration.name);
            let mut handler = declaration.handler.clone().unwrap();
            if let Some(schema) =
                parameters.get(&(package.summary.name.clone(), declaration.name.clone()))
            {
                handler.parameters = schema.clone();
            }
            let operation = declaration
                .handler
                .as_ref()
                .and_then(|handler| handler.operation);
            let script: Arc<dyn Tool> = Arc::new(Script {
                ingress: ingress.clone(),
                caller: invocation.caller,
                package: package.clone(),
                context: plugin::Context {
                    invocation: None,
                    turn: Some(invocation.turn.id),
                    surface: plugin::desktop::Surface::Workspace,
                    package: package.summary.reference(),
                    worktree: Some(invocation.worktree),
                    session: Some(invocation.turn.session),
                },
                message: invocation
                    .message
                    .text
                    .chars()
                    .take(
                        declaration
                            .handler
                            .as_ref()
                            .and_then(|handler| handler.flow.as_ref())
                            .and_then(|flow| flow.message_chars)
                            .unwrap_or(0),
                    )
                    .collect(),
                flows: declaration
                    .handler
                    .as_ref()
                    .and_then(|handler| handler.flow.as_ref())
                    .map(|flow| {
                        flow.operations
                            .iter()
                            .map(|operation| {
                                (
                                    *operation,
                                    operations::HostOperation::new(
                                        ingress,
                                        invocation,
                                        package,
                                        declaration,
                                        *operation,
                                        stop,
                                    ),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                name: name.clone(),
                description: declaration
                    .description_on(std::env::consts::OS)
                    .map(str::to_owned)
                    .expect("validated handler description"),
                handler: handler.clone(),
                mode: invocation.turn.config.mode,
                delegate: (operation == Some(tool::Operation::DelegateAgent)).then(|| {
                    super::super::delegation::Delegate::new(
                        ingress,
                        tasks,
                        invocation,
                        stop,
                        name.clone(),
                        declaration.description.clone().unwrap_or_default(),
                        handler.parameters.clone(),
                    )
                }),
                operation: operation
                    .filter(|operation| *operation != tool::Operation::DelegateAgent)
                    .map(|operation| {
                        operations::HostOperation::new(
                            ingress,
                            invocation,
                            package,
                            declaration,
                            operation,
                            stop,
                        )
                    }),
                turn: turns.get(&package.summary.name).cloned(),
                stop: stop.clone(),
            });
            let mut registration = if operation
                .is_some_and(|operation| operation != tool::Operation::DelegateAgent)
                || declaration
                    .handler
                    .as_ref()
                    .is_some_and(|handler| handler.flow.is_some())
            {
                ToolRegistration::new(script, declaration.presentation)
            } else if invocation.turn.config.mode == sailry_protocol::WorkMode::Plan
                && operation.is_none()
            {
                // Private Code workflows are managed, but plain handlers still need Plan filtering.
                ToolRegistration::new(script, declaration.presentation)
            } else {
                ToolRegistration::managed(script)
            };
            registration.presentation = declaration.presentation;
            registration.grouping = declaration.grouping;
            registration.plugin = Some(package.summary.name.clone());
            registration.display = declaration
                .display
                .clone()
                .map(|display| (registration.tool.name().into(), display));
            registration
        })
        .collect();
    Ok(contribution)
}

struct Script {
    ingress: Arc<Ingress>,
    caller: NodeId,
    package: plugin::Info,
    context: plugin::Context,
    name: String,
    description: String,
    handler: tool::Handler,
    mode: sailry_protocol::WorkMode,
    operation: Option<operations::HostOperation>,
    delegate: Option<super::super::delegation::Delegate>,
    flows: Vec<(tool::Operation, operations::HostOperation)>,
    message: String,
    turn: Option<Arc<turn::Turn>>,
    stop: CancellationToken,
}

#[async_trait]
impl Tool for Script {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(self.handler.parameters.clone())
    }
    fn is_agent_delegation(&self) -> bool {
        self.delegate.is_some()
    }
    fn is_read_only(&self) -> bool {
        policy::read_only(
            &self.handler,
            &self.package.extension.as_ref().unwrap().actions,
        )
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if self.handler.flow.is_some() {
            return self.execute_flow(context, arguments).await;
        }
        let prepared = self
            .evaluate(context.clone(), &self.handler.name, arguments.clone())
            .await?;
        if prepared.get("isError").and_then(Value::as_bool) == Some(true) {
            return Ok(prepared);
        }
        // The VM and its pool permit have been released before waiting on Node work.
        let mut output = if let Some(delegate) = &self.delegate {
            delegate
                .execute_prepared(context.clone(), prepared, arguments.clone())
                .await?
        } else if let Some(operation) = &self.operation {
            operation
                .execute_bound(context.clone(), prepared, arguments.clone())
                .await?
        } else {
            return Ok(prepared);
        };
        let media = media::Media::take(&mut output);
        let Some(handler) = &self.handler.result else {
            return Ok(media.restore(output));
        };
        if output.get("isError").and_then(Value::as_bool) == Some(true) {
            // A formatter can add presentation, but cannot replace the Node's failure.
            // Formatting never dispatches the completed operation again.
            if let Ok(formatted) = self
                .evaluate(
                    context,
                    handler,
                    json!({"arguments":arguments, "output":output}),
                )
                .await
            {
                if let Some(content) = formatted.get("sailry_content") {
                    output["sailry_content"] = content.clone();
                }
                if tool::ResultDisplay::from_value(&formatted).is_some() {
                    output["sailry_result"] = formatted["sailry_result"].clone();
                }
            }
            return Ok(output);
        }
        match self
            .evaluate(
                context,
                handler,
                json!({"arguments":arguments, "output":output}),
            )
            .await
        {
            Ok(result) => Ok(media.restore(result)),
            // The operation has completed. Preserve its result; never replay it to repair formatting.
            Err(error) => {
                Ok(media.restore(json!({"output":output, "presentation_error":error.to_string()})))
            }
        }
    }
}

impl Script {
    async fn evaluate(
        &self,
        context: Arc<dyn ToolContext>,
        export: &str,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if Some(context.session_id()) != self.context.session.map(|id| id.to_string()).as_deref() {
            return Err(AdkError::tool("plugin tool belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("plugin tool was cancelled"));
        }
        if serde_json::to_vec(&arguments)
            .map_err(|_| AdkError::tool("invalid tool arguments"))?
            .len()
            > plugin::host::MAX_DATA_BYTES
        {
            return Err(AdkError::tool("plugin tool input exceeds limit"));
        }
        let permit = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Err(AdkError::tool("plugin tool was cancelled")),
            permit = self.ingress.plugins.scripts.clone().acquire_owned() => permit.map_err(|_| AdkError::tool("plugin VM pool is closed"))?,
        };
        let ingress = self.ingress.clone();
        let caller = self.caller;
        let package = self.package.clone();
        let scope = self.context.clone();
        let call_id = identity::call(
            scope
                .turn
                .ok_or_else(|| AdkError::tool("plugin tool has no turn"))?,
            &scope.package,
            &self.name,
            context.function_call_id(),
        )?;
        let handler = export.to_owned();
        let turn = self.turn.clone();
        let stop = self.stop.clone();
        let mode = self.mode;
        let read_only = self.is_read_only();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut turn_state = turn
                .as_ref()
                .map(|turn| turn.state.lock())
                .transpose()
                .map_err(|_| AdkError::tool("plugin turn state is unavailable"))?;
            let bundle = ingress
                .plugins
                .read_host(&package, stop.clone())
                .map_err(|fault| AdkError::tool(fault.message))?;
            let state = Arc::new(Mutex::new(crate::plugins::script::execution::State {
                call_id: Some(call_id),
                mode,
                read_only,
                turn: turn_state.as_deref().cloned(),
                staged_turn: None,
            }));
            let result = crate::plugins::script::run_tool(
                bundle,
                &handler,
                arguments,
                crate::plugins::script::Environment {
                    context: scope,
                    target: ingress.node,
                    stop,
                },
                move |request| {
                    runtime.block_on(async {
                        let admitted = ingress.dispatch(caller, request).await?;
                        admitted.completion.await.map_err(|_| {
                            Fault::new(
                                ErrorCode::OutcomeUnknown,
                                "plugin SDK completion is unknown",
                            )
                        })?
                    })
                },
                state.clone(),
            )
            .map_err(|fault| AdkError::tool(fault.message))?;
            if let Some(staged) = state
                .lock()
                .map_err(|_| AdkError::tool("plugin turn state is unavailable"))?
                .staged_turn
                .take()
                && let Some(current) = turn_state.as_mut()
            {
                **current = staged;
            }
            if context.is_cancelled() {
                return Err(AdkError::tool("plugin tool was cancelled"));
            }
            Ok(result)
        })
        .await
        .map_err(|_| AdkError::tool("plugin tool worker failed"))?
    }
}
