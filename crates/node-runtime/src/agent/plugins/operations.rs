//! Declared host operations run without a controller-side script or MCP process.
use super::*;
use crate::agent::catalog::CallContext;
use goose_agent::tool::ToolProvider;
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, Request, plugin, tool::Operation};
use serde_json::{Value, json};

mod artifacts;
mod code;
mod commands;
pub(super) mod connections;
mod files;
mod knowledge;
mod models;
mod progress;
mod repository;
mod roles;
mod schema;
#[cfg(test)]
mod tests;
pub(super) use crate::plugins::tools::alias;

pub(super) fn bind(
    ingress: &Arc<Ingress>,
    tasks: &Tasks,
    invocation: &Invocation,
    stop: &CancellationToken,
) -> Vec<ToolRegistration> {
    let mut tools = Vec::new();
    let mut search_bound = false;
    for package in &invocation.plugins {
        let Some(extension) = &package.extension else {
            continue;
        };
        for declaration in &extension.tools {
            let Some(operation) = declaration.operation else {
                continue;
            };
            if !declaration.valid()
                || !super::selection::declared(invocation, package, declaration)
                || !crate::plugins::conversation::selects(
                    invocation.turn.config.assistant.as_ref(),
                    &invocation.plugins,
                    package,
                    &declaration.name,
                )
                || (invocation.connections.bound.is_some()
                    && invocation.turn.config.assistant.is_none()
                    && declaration.contexts.is_empty()
                    && !artifacts::handles(operation))
                || !declaration.available_on(std::env::consts::OS)
                || !declaration.available_in(invocation.project, invocation.connections.bound)
                || !available(ingress, invocation, operation)
                || (operation == Operation::SearchWeb && search_bound)
            {
                continue;
            }
            search_bound |= operation == Operation::SearchWeb;
            let name = alias(&package.summary.name, &declaration.name);
            let (descriptor, tool): (rmcp::model::Tool, Arc<dyn ToolProvider<CallContext>>) =
                if operation == Operation::DelegateAgent {
                    let tool = Arc::new(super::super::delegation::Delegate::new(
                        ingress,
                        tasks,
                        invocation,
                        stop,
                        name,
                        declaration.description.clone().unwrap_or_default(),
                        json!({"type":"object","properties":{"session":{"type":"string"},"role":{"type":["string","null"]},"task":{"type":"string"},"title":{"type":"string"},"background":{"type":"boolean"},"worktree":{"type":["string","null"]}},"required":["task"],"additionalProperties":false}),
                    ));
                    (tool.descriptor(), tool)
                } else if matches!(operation, Operation::GuideAgent | Operation::WaitAgent) {
                    let tool = Arc::new(super::super::delegation::Control::new(
                        ingress,
                        invocation,
                        package,
                        stop,
                        operation,
                        super::super::delegation::Delegate::new(
                            ingress,
                            tasks,
                            invocation,
                            stop,
                            name,
                            declaration.description.clone().unwrap_or_default(),
                            Value::Null,
                        ),
                    ));
                    (tool.descriptor(), tool)
                } else if operation == Operation::Progress {
                    let tool = Arc::new(progress::UpdatePlan {
                        name,
                        description: declaration.description.clone(),
                        session: invocation.turn.session.to_string(),
                        stop: stop.clone(),
                    });
                    (tool.descriptor(), tool)
                } else {
                    let mut tool = HostOperation::new(
                        ingress,
                        invocation,
                        package,
                        declaration,
                        operation,
                        stop,
                    );
                    tool.managed = true;
                    let tool = Arc::new(tool);
                    (tool.descriptor(), tool)
                };
            let mut registration = if matches!(
                operation,
                Operation::DelegateAgent
                    | Operation::GuideAgent
                    | Operation::WaitAgent
                    | Operation::WriteArtifact
                    | Operation::RunSsh
                    | Operation::TransferSsh
                    | Operation::ExecuteDatabase
                    | Operation::SaveDatabaseConnection
                    | Operation::SaveSshConnection
            ) {
                ToolRegistration::managed(descriptor, tool)
            } else {
                ToolRegistration::new(descriptor, tool, declaration.presentation)
            };
            registration.delegation = operation == Operation::DelegateAgent;
            registration.presentation = declaration.presentation;
            registration.grouping = declaration.grouping;
            registration.plugin = Some(package.summary.name.clone());
            registration.display = declaration.display.clone().map(|display| {
                (
                    registration.tool.name.to_string(),
                    connections::display(operation, display, &invocation.connections),
                )
            });
            tools.push(registration);
        }
    }
    tools
}

pub(super) fn available(ingress: &Ingress, invocation: &Invocation, operation: Operation) -> bool {
    match operation {
        Operation::ReadComputer | Operation::ControlComputer => false,
        Operation::WriteArtifact => invocation.child.is_none(),
        operation if !operation.current() => false,
        operation if code::handles(operation) => ingress.code_index.enabled(),
        Operation::SearchWeb => {
            ingress.search.available() && !super::model_tools::preferred(ingress, invocation)
        }
        Operation::DelegateAgent | Operation::GuideAgent | Operation::WaitAgent => {
            invocation.child.is_none() && invocation.connections.bound.is_none()
        }
        Operation::InspectMedia => invocation
            .media
            .contains_key(&sailry_protocol::media::Kind::Vision),
        Operation::GenerateImage => invocation
            .media
            .contains_key(&sailry_protocol::media::Kind::Image),
        Operation::GenerateVideo => invocation
            .media
            .contains_key(&sailry_protocol::media::Kind::Video),
        Operation::ReadBrowser | Operation::ControlBrowser => {
            ingress.browsers.available(invocation.caller)
        }
        Operation::ReadExternalBrowser | Operation::ControlExternalBrowser => {
            ingress.external_browser.available()
        }
        _ => true,
    }
}

pub(super) struct HostOperation {
    ingress: Arc<Ingress>,
    transport: Arc<dyn Transport>,
    turn: TurnId,
    caller: sailry_protocol::NodeId,
    context: plugin::Context,
    name: String,
    description: String,
    operation: Operation,
    settings: Option<Result<Value, Fault>>,
    connections: Vec<sailry_protocol::connection::Resource>,
    stop: CancellationToken,
    managed: bool,
}

impl HostOperation {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(schema::parameters(self.operation))
    }
    fn is_read_only(&self) -> bool {
        self.operation.read_only()
    }
    async fn execute(&self, context: Arc<CallContext>, arguments: Value) -> Result<Value, Fault> {
        if Some(context.session_id()) != self.context.session.map(|id| id.to_string()).as_deref() {
            return Err(Fault::invalid("plugin tool belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(Fault::invalid("plugin tool was cancelled"));
        }
        if self.operation == Operation::ReadSettings {
            return self
                .settings
                .clone()
                .unwrap_or_else(|| {
                    Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "plugin configuration is unavailable",
                    ))
                })
                .map_err(|fault| Fault::invalid(fault.message));
        }
        let output = self
            .execute_bound(context, arguments.clone(), arguments)
            .await?;
        if output.get("error").is_some() {
            Ok(output)
        } else {
            let mut data = output.get("data").cloned().unwrap_or(Value::Null);
            for key in ["sailry_context", "sailry_guide_loaded"] {
                if let Some(value) = output.get(key)
                    && data.is_object()
                {
                    data[key] = value.clone();
                }
            }
            Ok(data)
        }
    }
}

impl HostOperation {
    pub(super) fn new(
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        package: &plugin::Info,
        declaration: &sailry_protocol::tool::Declaration,
        operation: Operation,
        stop: &CancellationToken,
    ) -> Self {
        Self {
            ingress: ingress.clone(),
            transport: Arc::new(Local::new(ingress.node, invocation.caller, ingress.clone())),
            turn: invocation.turn.id,
            caller: invocation.caller,
            context: plugin::Context {
                invocation: None,
                turn: Some(invocation.turn.id),
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(invocation.worktree),
                session: Some(invocation.turn.session),
            },
            name: alias(&package.summary.name, &declaration.name),
            description: declaration
                .description_on(std::env::consts::OS)
                .map(str::to_owned)
                .unwrap_or_else(|| schema::description(operation).into()),
            operation,
            settings: invocation
                .plugin_settings
                .get(&package.summary.name)
                .map(|result| {
                    result
                        .as_ref()
                        .map(|settings| json!({"values": settings.values}))
                        .map_err(Clone::clone)
                }),
            connections: invocation.connections.resources(),
            stop: stop.clone(),
            managed: false,
        }
    }

    pub(super) async fn prepare(&self, arguments: Value) -> Result<Request, Fault> {
        let command = if connections::handles(self.operation) {
            let (target, action) = connections::action(self.operation, arguments)?;
            self.ingress
                .connection_command(
                    self.turn,
                    self.context.package.clone(),
                    target,
                    action,
                    self.connections.clone(),
                )
                .await?
        } else if repository::handles(self.operation) {
            let registered = self.ingress.agent_worktrees(self.turn).await?;
            repository::command(
                self.operation,
                arguments,
                self.context
                    .worktree
                    .ok_or_else(|| Fault::invalid("repository operation requires a worktree"))?,
                &registered,
            )?
        } else {
            schema::command(self.operation, arguments, &self.context)?
        };
        Ok(Request::new(self.transport.target(), command).with_plugin(self.context.clone()))
    }

    pub(super) async fn authorize(
        &self,
        context: &CallContext,
        arguments: Value,
        prepared: Option<Command>,
    ) -> Result<sailry_protocol::RequestId, Fault> {
        self.ingress
            .authorize(self.turn, context.request_with(arguments), prepared)
            .await
    }

    pub(super) async fn dispatch(
        &self,
        context: &CallContext,
        request: Request,
    ) -> Result<Value, Fault> {
        if connections::handles(self.operation)
            && !matches!(
                self.operation,
                Operation::ListDatabaseConnections
                    | Operation::SaveDatabaseConnection
                    | Operation::ListSshConnections
                    | Operation::SaveSshConnection
            )
        {
            let cancel = match request.command {
                Command::BrowseDatabase { .. } | Command::QueryDatabase { .. } => {
                    Command::CancelDatabase {
                        request: request.id,
                    }
                }
                Command::RunSsh { .. } | Command::TransferSsh { .. } => Command::CancelSsh {
                    request: request.id,
                },
                _ => unreachable!(),
            };
            let admission = self.transport.dispatch(request).await?;
            let output = tokio::select! {
                biased;
                _ = self.stop.cancelled() => {
                    let _ = self.transport.dispatch(Request::new(self.transport.target(),cancel)).await;
                    return Err(Fault::new(ErrorCode::OutcomeUnknown,"connection operation was cancelled; inspect its outcome before retrying"));
                }
                result = admission.completion => result.map_err(|_| Fault::new(ErrorCode::OutcomeUnknown,"connection operation outcome is unavailable"))??,
            };
            return serde_json::to_value(output)
                .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()));
        }
        let workspace = matches!(
            request.command,
            Command::RunCommand {
                access: sailry_protocol::process::Access::Workspace,
                ..
            }
        );
        let command_record = match &request.command {
            Command::RunCommand {
                background: false, ..
            } => self.context.session.map(|session| (session, request.id)),
            Command::ReadCommand { session, id } => Some((*session, *id)),
            _ => None,
        };
        let work = async {
            let admission = self.transport.dispatch(request).await?;
            let output = admission.completion.await.map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "plugin tool response is unavailable",
                )
            })??;
            if let Some(failure) = commands::failure(&output)
                && let Some(timeline) = self.ingress.agents.timeline(self.turn)
            {
                timeline.fail_tool(
                    context.branch(),
                    context.function_call_id(),
                    &self.name,
                    failure.code,
                );
            }
            if self.operation == Operation::ReadCommand
                && let sailry_protocol::Output::CommandOutput(snapshot) = &output
            {
                self.ingress
                    .collect_command(self.turn, snapshot.clone())
                    .await?;
            }
            if roles::handles(self.operation) {
                return roles::output(output);
            }
            if self.operation == Operation::ListWorktrees {
                let registered = self.ingress.agent_worktrees(self.turn).await?;
                return repository::output(
                    output,
                    self.context.worktree.expect("captured worktree"),
                    &registered,
                );
            }
            if self.operation == Operation::StorageTransaction {
                return Ok(plugin::transaction::sdk::output(output));
            }
            let mut value = serde_json::to_value(&output)
                .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))?;
            if connections::handles(self.operation) {
                connections::annotate(self, &output, &mut value).await?;
            }
            commands::annotate(&output, &mut value, workspace);
            if let Some((session, id)) = command_record {
                commands::retain(&self.ingress.commands, session, id, &output, &mut value).await;
            }
            artifacts::annotate(&output, &mut value);
            Ok(value)
        };
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled,"plugin tool was cancelled")),
            result = work => result,
        }
    }

    pub(super) async fn execute_bound(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
        authorized_arguments: Value,
    ) -> Result<Value, Fault> {
        let work = async {
            let mut request = self.prepare(arguments.clone()).await?;
            if let Command::RunCommand { access, .. } = &mut request.command {
                let requested: sailry_protocol::process::Access = authorized_arguments
                    .get("access")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|_| Fault::invalid("invalid command access"))?
                    .unwrap_or_default();
                if *access != requested {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "prepared command access differs from approval",
                    ));
                }
                *access = self
                    .ingress
                    .approved_command_access(
                        self.turn,
                        context.request_with(authorized_arguments.clone()),
                    )
                    .await?;
            }
            let ssh_trust = if connections::ssh(self.operation)
                || (connections::handles(self.operation) && !self.is_read_only() && self.managed)
            {
                let trust = if connections::ssh(self.operation) {
                    connections::preflight(self, &request).await?
                } else {
                    None
                };
                let decision = tokio::select! {
                    biased;
                    _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled,"connection approval was cancelled")),
                    decision = self.ingress.confirm_connection(self.turn, context.request_with(authorized_arguments.clone()), crate::store::agent::ApprovalBinding { owner: self.context.package.clone(), tool: self.name.clone(), operation: Some(self.operation) }, request.command.clone(), trust.clone()) => decision?,
                };
                if !matches!(
                    decision,
                    sailry_protocol::conversation::Decision::Approve
                        | sailry_protocol::conversation::Decision::ApproveSession
                        | sailry_protocol::conversation::Decision::ApprovePrefix(_)
                ) {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "connection operation was denied",
                    ));
                }
                trust
            } else {
                None
            };
            if !self.is_read_only() {
                if self.operation == Operation::WriteArtifact {
                    self.ingress
                        .confirm(
                            self.turn,
                            context.request_with(authorized_arguments.clone()),
                            Some(crate::store::agent::ApprovalBinding {
                                owner: self.context.package.clone(),
                                tool: self.name.clone(),
                                operation: Some(self.operation),
                            }),
                        )
                        .await?;
                }
                let id = self
                    .authorize(
                        context.as_ref(),
                        authorized_arguments,
                        Some(request.command.clone()),
                    )
                    .await?;
                if let Some(trust) = ssh_trust {
                    let revision = connections::trust(self, &trust).await?;
                    match &mut request.command {
                        Command::RunSsh {
                            expected_revision, ..
                        }
                        | Command::TransferSsh {
                            expected_revision, ..
                        } => *expected_revision = revision,
                        _ => {
                            return Err(Fault::invalid(
                                "host key approval requires an SSH command",
                            ));
                        }
                    }
                }
                request.id = id;
                if matches!(
                    request.command,
                    Command::WriteFile { .. }
                        | Command::EditFile { .. }
                        | Command::EditNotebook { .. }
                ) {
                    self.ingress
                        .checkpoint_write(self.turn, &request, self.stop.clone())
                        .await?;
                }
            }
            let mut cancellation = code::Cancellation::new(&self.ingress, self.caller, &request);
            let mut result = self.dispatch(context.as_ref(), request).await;
            if let Some(cancellation) = cancellation.as_mut() {
                cancellation.finish();
            }
            if let Ok(output) = &mut result
                && let Some(worktree) = self.context.worktree
            {
                crate::agent::prompt::observe(
                    &self.ingress,
                    worktree,
                    self.operation,
                    context.as_ref(),
                    output,
                    &self.stop,
                )
                .await?;
            }
            result
        };
        Ok(match work.await {
            Ok(output) => output,
            Err(error) => {
                if let Some(timeline) = self.ingress.agents.timeline(self.turn) {
                    timeline.fail_tool(
                        context.branch(),
                        context.function_call_id(),
                        &self.name,
                        Some(error.code),
                    );
                }
                files::failure(self.operation, error)
            }
        })
    }
}

crate::agent::catalog::provider!(HostOperation);
