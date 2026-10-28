//! Package tools share the bounded headless VM and the original Agent history.
use super::*;
use crate::agent::catalog::CallContext;
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
                        && super::selection::declared(invocation, package, tool)
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
        if initialized.tools.is_empty() {
            continue;
        }
        if let Some(callback) = initialized.owner.observer(contribution.completion.clone()) {
            contribution.callbacks.push((10, callback));
        }
        contribution
            .instructions
            .insert(package.summary.name.clone(), initialized.instruction);
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
            let script = Arc::new(Script {
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
            let descriptor = script.descriptor();
            let delegation = script.is_agent_delegation();
            let mut registration = if matches!(
                operation,
                Some(tool::Operation::RunSsh | tool::Operation::TransferSsh)
            ) {
                ToolRegistration::managed(descriptor, script)
            } else if operation.is_some_and(|operation| operation != tool::Operation::DelegateAgent)
                || declaration
                    .handler
                    .as_ref()
                    .is_some_and(|handler| handler.flow.is_some())
            {
                ToolRegistration::new(descriptor, script, declaration.presentation)
            } else if invocation.turn.config.mode == sailry_protocol::WorkMode::Plan
                && operation.is_none()
            {
                // Plain handlers still pass through the runtime's Plan authority check.
                ToolRegistration::new(descriptor, script, declaration.presentation)
            } else {
                ToolRegistration::managed(descriptor, script)
            };
            registration.delegation = delegation;
            registration.presentation = declaration.presentation;
            registration.grouping = declaration.grouping;
            registration.plugin = Some(package.summary.name.clone());
            registration.display = declaration.display.clone().map(|display| {
                let display = match operation {
                    Some(operation) => operations::connections::display(
                        operation,
                        display,
                        &invocation.connections,
                    ),
                    None => display,
                };
                (registration.tool.name.to_string(), display)
            });
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

impl Script {
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
    async fn execute(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<rmcp::model::CallToolResult, Fault> {
        if self.handler.flow.is_some() {
            return crate::agent::catalog::json(self.execute_flow(context, arguments).await?);
        }
        let prepared = self
            .evaluate(context.clone(), &self.handler.name, arguments.clone())
            .await?;
        if prepared.get("isError").and_then(Value::as_bool) == Some(true) {
            return crate::agent::catalog::json(prepared);
        }
        // The VM has been released before waiting on Node work.
        let mut meta = None;
        let mut output = if let Some(delegate) = &self.delegate {
            let result = delegate
                .execute_prepared(context.clone(), prepared, arguments.clone())
                .await?;
            meta = result.meta;
            result
                .structured_content
                .ok_or_else(|| Fault::invalid("child result is unavailable"))?
        } else if let Some(operation) = &self.operation {
            operation
                .execute_bound(context.clone(), prepared, arguments.clone())
                .await?
        } else {
            return crate::agent::catalog::json(prepared);
        };
        let media = media::Media::take(&mut output);
        let Some(handler) = &self.handler.result else {
            return formatted(media.restore(output), meta);
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
            return formatted(output, meta);
        }
        match self
            .evaluate(
                context,
                handler,
                json!({"arguments":arguments, "output":output}),
            )
            .await
        {
            Ok(result) => formatted(media.restore(result),meta),
            // The operation has completed. Preserve its result; never replay it to repair formatting.
            Err(error) => {
                formatted(media.restore(json!({"output":output, "presentation_error":super::failures::presentation(&error)})),meta)
            }
        }
    }
}

impl Script {
    async fn evaluate(
        &self,
        context: Arc<CallContext>,
        export: &str,
        arguments: Value,
    ) -> Result<Value, Fault> {
        if Some(context.session_id()) != self.context.session.map(|id| id.to_string()).as_deref() {
            return Err(Fault::invalid("plugin tool belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(Fault::invalid("plugin tool was cancelled"));
        }
        if serde_json::to_vec(&arguments)
            .map_err(|_| Fault::invalid("invalid tool arguments"))?
            .len()
            > plugin::host::MAX_DATA_BYTES
        {
            return Err(Fault::invalid("plugin tool input exceeds limit"));
        }
        let ingress = self.ingress.clone();
        let caller = self.caller;
        let package = self.package.clone();
        let scope = self.context.clone();
        let call_id = identity::call(
            scope
                .turn
                .ok_or_else(|| Fault::invalid("plugin tool has no turn"))?,
            &scope.package,
            &self.name,
            context.function_call_id(),
        )?;
        let handler = export.to_owned();
        let turn = self.turn.clone();
        let stop = self.stop.clone();
        let mode = self.mode;
        let read_only = self.is_read_only();
        let card_scope = scope.clone();
        let card_package = package.clone();
        let node = ingress.node;
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            let mut turn_state = turn
                .as_ref()
                .map(|turn| turn.state.lock())
                .transpose()
                .map_err(|_| Fault::invalid("plugin turn state is unavailable"))?;
            let bundle = ingress
                .plugins
                .read_host(&package, stop.clone())
                .map_err(error)?;
            let state = Arc::new(Mutex::new(crate::plugins::script::execution::State {
                call_id: Some(call_id),
                mode,
                read_only,
                turn: turn_state.as_deref().cloned(),
                staged_turn: None,
            }));
            let consumed = Arc::new(Mutex::new(Vec::new()));
            let reads = consumed.clone();
            let collector = ingress.clone();
            let collect_runtime = runtime.clone();
            let parent = card_scope.turn.expect("agent tool has a captured turn");
            let deadline = ingress.plugins.script_deadline();
            let stopped = stop.clone();
            let reporter = ingress.clone();
            let report_runtime = runtime.clone();
            let mut result = crate::plugins::script::run_tool(
                bundle,
                &handler,
                arguments,
                crate::plugins::script::Environment {
                    context: scope,
                    target: ingress.node,
                    stop,
                    deadline,
                },
                move |request| {
                    runtime.block_on(async {
                        let output = ingress
                            .script_dispatch(caller, request, deadline, &stopped)
                            .await?;
                        if let sailry_protocol::Output::CommandOutput(snapshot) = &output {
                            reads
                                .lock()
                                .map_err(|_| {
                                    Fault::new(
                                        ErrorCode::Internal,
                                        "command read tracking is unavailable",
                                    )
                                })?
                                .push(snapshot.clone());
                        }
                        Ok(output)
                    })
                },
                state.clone(),
            )
            .map_err(|fault| {
                let fault =
                    report_runtime.block_on(reporter.script_failure(caller, fault, &package));
                error(fault)
            })?;
            crate::plugins::cards::stamp(&mut result, node, &card_scope, &card_package, call_id);
            if let Some(staged) = state
                .lock()
                .map_err(|_| Fault::invalid("plugin turn state is unavailable"))?
                .staged_turn
                .take()
                && let Some(current) = turn_state.as_mut()
            {
                **current = staged;
            }
            if context.is_cancelled() {
                return Err(Fault::invalid("plugin tool was cancelled"));
            }
            if result.get("isError").and_then(Value::as_bool) != Some(true) {
                let snapshots = std::mem::take(
                    &mut *consumed
                        .lock()
                        .map_err(|_| Fault::invalid("command read tracking is unavailable"))?,
                );
                for snapshot in snapshots {
                    collect_runtime.block_on(collector.collect_command(parent, snapshot))?;
                }
            }
            Ok(result)
        })
        .await
        .map_err(|_| Fault::invalid("plugin tool worker failed"))?
    }
}

fn formatted(
    value: Value,
    meta: Option<rmcp::model::MetaObject>,
) -> Result<rmcp::model::CallToolResult, Fault> {
    let mut result = crate::agent::catalog::json(value)?;
    if let Some(meta) = meta {
        result
            .meta
            .get_or_insert_with(Default::default)
            .extend(meta);
    }
    Ok(result)
}

fn error(fault: Fault) -> Fault {
    fault
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_fault_survives_agent_projection() {
        use sailry_protocol::{
            FaultDetails, RequestId,
            notification::{DiagnosticPhase, DiagnosticReason, SystemDiagnostic, SystemSource},
        };
        let mut fault = Fault::new(
            ErrorCode::OutcomeUnknown,
            "plugin SDK completion is unknown",
        );
        let mut diagnostic =
            SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
        diagnostic.reason = DiagnosticReason::Timeout;
        diagnostic.phase = DiagnosticPhase::Sdk;
        diagnostic.operation = Some("write_plugin_value".into());
        diagnostic.request = Some(RequestId::new());
        fault.details = Some(FaultDetails::SystemDiagnostic(Box::new(diagnostic)));
        let projected = error(fault.clone());
        assert_eq!(projected, fault);
    }
}

crate::agent::catalog::provider!(Script, crate::agent::catalog::result);
