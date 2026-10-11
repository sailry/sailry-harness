//! Declared host operations run without a controller-side script or MCP process.
use super::*;
use adk_core::Tool;
use adk_core::{AdkError, ToolContext};
use async_trait::async_trait;
use sailry_link::{Local, Transport};
use sailry_protocol::{Command, Request, plugin, tool::Operation};
use serde_json::{Value, json};

mod artifacts;
mod commands;
mod connections;
mod files;
mod knowledge;
mod progress;
mod repository;
mod schema;
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
            let tool: Arc<dyn Tool> = if operation == Operation::DelegateAgent {
                Arc::new(super::super::delegation::Delegate::new(
                    ingress,
                    tasks,
                    invocation,
                    stop,
                    name,
                    declaration.description.clone().unwrap_or_default(),
                    json!({"type":"object","properties":{"session":{"type":"string"},"role":{"type":["string","null"]},"task":{"type":"string"},"title":{"type":"string"},"background":{"type":"boolean"},"worktree":{"type":["string","null"]}},"required":["task"],"additionalProperties":false}),
                ))
            } else if matches!(operation, Operation::GuideAgent | Operation::WaitAgent) {
                Arc::new(super::super::delegation::Control::new(
                    ingress,
                    invocation,
                    package,
                    stop,
                    operation,
                    name,
                    declaration.description.clone().unwrap_or_default(),
                ))
            } else if operation == Operation::Progress {
                Arc::new(progress::UpdatePlan {
                    name,
                    description: declaration.description.clone(),
                    session: invocation.turn.session.to_string(),
                    stop: stop.clone(),
                })
            } else {
                Arc::new(HostOperation::new(
                    ingress,
                    invocation,
                    package,
                    declaration,
                    operation,
                    stop,
                ))
            };
            let mut registration = if matches!(
                operation,
                Operation::DelegateAgent
                    | Operation::GuideAgent
                    | Operation::WaitAgent
                    | Operation::WriteArtifact
                    | Operation::RunSsh
                    | Operation::TransferSsh
            ) {
                ToolRegistration::managed(tool)
            } else {
                ToolRegistration::new(tool, declaration.presentation)
            };
            registration.presentation = declaration.presentation;
            registration.grouping = declaration.grouping;
            registration.plugin = Some(package.summary.name.clone());
            registration.display = declaration
                .display
                .clone()
                .map(|display| (registration.tool.name().into(), display));
            tools.push(registration);
        }
    }
    tools
}

pub(super) fn available(ingress: &Ingress, invocation: &Invocation, operation: Operation) -> bool {
    match operation {
        Operation::WriteArtifact => invocation.child.is_none(),
        operation
            if crate::code::handles(operation) || crate::code::management::handles(operation) =>
        {
            ingress.code.enabled()
        }
        Operation::SearchWeb => {
            ingress.search.available() && !super::model_tools::preferred(ingress, invocation)
        }
        Operation::DelegateAgent | Operation::GuideAgent | Operation::WaitAgent => {
            invocation.child.is_none() && invocation.project.is_some()
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
}

#[async_trait]
impl Tool for HostOperation {
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
    fn is_concurrency_safe(&self) -> bool {
        self.is_read_only()
            && !matches!(
                self.operation,
                Operation::ReadExternalBrowser
                    | Operation::ControlExternalBrowser
                    | Operation::ReadBrowser
                    | Operation::ControlBrowser
            )
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if Some(context.session_id()) != self.context.session.map(|id| id.to_string()).as_deref() {
            return Err(AdkError::tool("plugin tool belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("plugin tool was cancelled"));
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
                .map_err(|fault| AdkError::tool(fault.message));
        }
        let output = self
            .execute_bound(context, arguments.clone(), arguments)
            .await?;
        if output.get("error").is_some() {
            Ok(output)
        } else {
            Ok(output.get("data").cloned().unwrap_or(Value::Null))
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
            connections: invocation
                .connections
                .databases
                .iter()
                .map(|profile| sailry_protocol::connection::Resource::Database(profile.id))
                .chain(
                    invocation
                        .connections
                        .ssh
                        .iter()
                        .map(|profile| sailry_protocol::connection::Resource::Ssh(profile.id)),
                )
                .collect(),
            stop: stop.clone(),
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
        context: &dyn ToolContext,
        arguments: Value,
        prepared: Option<Command>,
    ) -> Result<sailry_protocol::RequestId, Fault> {
        self.ingress
            .authorize(
                self.turn,
                adk_core::ToolConfirmationRequest {
                    tool_name: self.name.clone(),
                    function_call_id: Some(context.function_call_id().into()),
                    args: arguments,
                },
                prepared,
            )
            .await
    }

    pub(super) async fn dispatch(&self, request: Request) -> Result<Value, Fault> {
        self.ingress
            .check_plugin(self.caller, request.clone())
            .await?;
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
        let work = async {
            let admission = self.transport.dispatch(request).await?;
            let output = admission.completion.await.map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "plugin tool response is unavailable",
                )
            })??;
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
            artifacts::annotate(&output, &mut value);
            if self.ingress.code.after_edit()
                && matches!(self.operation, Operation::WriteFile | Operation::EditFile)
                && let sailry_protocol::Output::FileWritten(written) = &output
                && let Some(worktree) = self.context.worktree
            {
                match self
                    .ingress
                    .code
                    .changed(worktree, std::slice::from_ref(&written.path), &self.stop)
                    .await
                {
                    Ok(diagnostics) if !diagnostics.is_empty() => {
                        value["data"]["diagnostics_after_edit"] =
                            json!({"title":"Diagnostics after edit:","diagnostics":diagnostics});
                    }
                    Ok(_) => {}
                    Err(error) => {
                        value["data"]["diagnostics_notice"] = json!(error.message);
                    }
                }
            }
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
        context: Arc<dyn ToolContext>,
        arguments: Value,
        authorized_arguments: Value,
    ) -> adk_core::Result<Value> {
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
                        adk_core::ToolConfirmationRequest {
                            tool_name: self.name.clone(),
                            function_call_id: Some(context.function_call_id().into()),
                            args: authorized_arguments.clone(),
                        },
                    )
                    .await?;
            }
            if !connections::management(self.operation) {
                self.ingress
                    .check_plugin(self.caller, request.clone())
                    .await?;
            }
            if let Command::ReadCode {
                worktree, query, ..
            } = &request.command
                && let Some(startup) = self.ingress.prepare_code(*worktree, query).await?
            {
                let confirmation = adk_core::ToolConfirmationRequest {
                    tool_name: self.name.clone(),
                    function_call_id: Some(context.function_call_id().into()),
                    args: authorized_arguments.clone(),
                };
                let binding = crate::store::agent::ApprovalBinding {
                    owner: self.context.package.clone(),
                    tool: self.name.clone(),
                    operation: Some(self.operation),
                };
                let decision = tokio::select! {biased;_ = self.stop.cancelled()=>return Err(Fault::new(ErrorCode::Cancelled,"language startup was cancelled")),decision=self.ingress.confirm_code(self.turn,confirmation,binding,startup.clone())=>decision?};
                if !matches!(
                    decision,
                    sailry_protocol::conversation::Decision::Approve
                        | sailry_protocol::conversation::Decision::ApproveSession
                        | sailry_protocol::conversation::Decision::ApprovePrefix(_)
                ) {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "language startup was denied",
                    ));
                }
                let command = Command::StartCodeServer { startup };
                let id = self
                    .authorize(
                        context.as_ref(),
                        authorized_arguments.clone(),
                        Some(command.clone()),
                    )
                    .await?;
                let mut start = Request::new(self.transport.target(), command)
                    .with_plugin(self.context.clone());
                start.id = id;
                let admission = self.transport.dispatch(start).await?;
                admission.completion.await.map_err(|_| {
                    Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "language startup outcome is unavailable",
                    )
                })??;
            }
            let ssh_trust = if connections::ssh(self.operation) {
                let trust = connections::preflight(self, &request).await?;
                let decision = tokio::select! {
                    biased;
                    _ = self.stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled,"SSH approval was cancelled")),
                    decision = self.ingress.confirm_ssh(self.turn, adk_core::ToolConfirmationRequest {
                        tool_name: self.name.clone(), function_call_id: Some(context.function_call_id().into()), args: authorized_arguments.clone(),
                    }, crate::store::agent::ApprovalBinding { owner: self.context.package.clone(), tool: self.name.clone(), operation: Some(self.operation) }, trust.clone()) => decision?,
                };
                if !matches!(
                    decision,
                    sailry_protocol::conversation::Decision::Approve
                        | sailry_protocol::conversation::Decision::ApproveSession
                        | sailry_protocol::conversation::Decision::ApprovePrefix(_)
                ) {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "SSH operation was denied",
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
                            adk_core::ToolConfirmationRequest {
                                tool_name: self.name.clone(),
                                function_call_id: Some(context.function_call_id().into()),
                                args: authorized_arguments.clone(),
                            },
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
                    connections::trust(self, &trust).await?;
                }
                if connections::handles(self.operation) && !connections::management(self.operation)
                {
                    request = self.prepare(arguments).await?;
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
            self.dispatch(request).await
        };
        Ok(match work.await {
            Ok(output) => output,
            Err(error) => files::failure(self.operation, error),
        })
    }
}
