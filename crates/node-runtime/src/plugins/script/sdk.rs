use super::*;
use rquickjs::{Ctx, prelude::MutFn};
use sailry_protocol::{Command, NodeId, Output, Request};
use serde_json::{Value as Json, json};

pub(super) fn install(
    ctx: &Ctx<'_>,
    target: NodeId,
    context: plugin::Context,
    control: Control,
    dispatch: impl FnMut(Request) -> sailry_link::Response + 'static,
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
) -> rquickjs::Result<()> {
    let mut bridge = Bridge {
        execution,
        target,
        context,
        control,
        dispatch,
        drafts: BTreeMap::new(),
        calls: 0,
        writes: 0,
        installations: std::collections::BTreeSet::new(),
    };
    ctx.globals().set(
        "__sailry",
        Function::new(
            ctx.clone(),
            MutFn::new(move |operation: String, body: String| {
                let result = bridge.call(&operation, &body);
                serde_json::to_string(&result).expect("JSON bridge values are serializable")
            }),
        )?,
    )
}

struct Bridge<F> {
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
    target: NodeId,
    context: plugin::Context,
    control: Control,
    dispatch: F,
    drafts: BTreeMap<String, Request>,
    calls: usize,
    writes: u64,
    installations: std::collections::BTreeSet<sailry_protocol::RequestId>,
}

impl<F: FnMut(Request) -> sailry_link::Response> Bridge<F> {
    fn call(&mut self, operation: &str, body: &str) -> Result<Json, Fault> {
        if self.control.stop.is_cancelled() || Instant::now() >= self.control.deadline {
            return Err(Fault::new(ErrorCode::Cancelled, "plugin callback stopped"));
        }
        if body.len() > plugin::host::MAX_DATA_BYTES || self.calls >= 1024 {
            return Err(sailry_protocol::Fault::invalid(
                "plugin callback SDK budget exhausted",
            ));
        }
        self.calls += 1;
        match operation {
            "id.new" => Ok(json!(sailry_protocol::RequestId::new())),
            "id.call" | "turn.read" | "turn.stage" => {
                let execution = self.execution.as_ref().ok_or_else(|| {
                    sailry_protocol::Fault::invalid("operation requires an Agent tool invocation")
                })?;
                let mut execution = execution.lock().map_err(|_| {
                    sailry_protocol::Fault::invalid("tool execution state is unavailable")
                })?;
                execution.call(operation, body)
            }
            "context" => {
                let mut value = json!(self.context);
                value["package"]["settings_revision"] =
                    json!(self.context.package.settings_revision.to_string());
                Ok(value)
            }
            "prepare" | "read" | "knowledge.create" | "models.prepare" | "models.register"
            | "models.list" => {
                let mut value: Json = serde_json::from_str(body)
                    .map_err(|_| sailry_protocol::Fault::invalid("invalid SDK command"))?;
                if operation == "models.list" {
                    #[derive(serde::Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Input {
                        target: sailry_protocol::installation::Target,
                        kind: Option<sailry_protocol::models::Kind>,
                    }
                    let input: Input = serde_json::from_value(value)
                        .map_err(|_| Fault::invalid("invalid model list"))?;
                    if input.target != sailry_protocol::installation::Target::Node
                        || input.kind == Some(sailry_protocol::models::Kind::Speech)
                    {
                        return Err(Fault::invalid("Node local model list required"));
                    }
                    value = json!(Command::ReadLocalModels { kind: input.kind });
                }
                if operation == "knowledge.create" {
                    let draft: sailry_protocol::knowledge::Draft = serde_json::from_value(value)
                        .map_err(|_| Fault::invalid("invalid knowledge collection draft"))?;
                    value = json!(Command::CreateKnowledgeCollection {
                        definition: draft.into_definition(self.context.worktree)?,
                        plugin: None,
                    });
                }
                if operation == "models.prepare" {
                    let model: sailry_protocol::installation::ModelRef =
                        serde_json::from_value(value)
                            .map_err(|_| Fault::invalid("invalid model installation"))?;
                    value = json!(model.node_command()?);
                }
                if operation == "models.register" {
                    let registration: sailry_protocol::installation::Registration =
                        serde_json::from_value(value)
                            .map_err(|_| Fault::invalid("invalid model registration"))?;
                    value = json!(registration.node_command()?);
                }
                let revision_path = if matches!(
                    value["kind"].as_str(),
                    Some("request_provider_http" | "execute_provider_http")
                ) {
                    "/data/expected_revision"
                } else {
                    "/data/package/settings_revision"
                };
                if let Some(revision) = value.pointer_mut(revision_path) {
                    let text = revision.as_str().ok_or_else(|| {
                        sailry_protocol::Fault::invalid("revision must be a decimal string")
                    })?;
                    let parsed = text
                        .parse::<u64>()
                        .map_err(|_| sailry_protocol::Fault::invalid("invalid revision"))?;
                    if parsed.to_string() != text {
                        return Err(sailry_protocol::Fault::invalid("invalid revision"));
                    }
                    *revision = json!(parsed);
                }
                if value["kind"] == "plugin_transaction" {
                    let operations =
                        plugin::transaction::sdk::decode(value["data"]["operations"].take())?;
                    value["data"]["operations"] = json!(operations);
                }
                if matches!(
                    value["kind"].as_str(),
                    Some(
                        "write_plugin_value"
                            | "write_plugin_conversation_value"
                            | "write_indexed_plugin_value"
                            | "remove_plugin_value"
                            | "remove_plugin_conversation_value"
                            | "submit_turn"
                            | "continue_turn"
                            | "rebuild_knowledge_collection"
                            | "delete_knowledge_collection"
                            | "start_connector_login"
                    )
                ) && let Some(revision) = value
                    .pointer_mut("/data/expected_revision")
                    .filter(|value| !value.is_null())
                {
                    let parsed = if let Some(text) = revision.as_str() {
                        text.parse::<u64>()
                            .map_err(|_| sailry_protocol::Fault::invalid("invalid revision"))?
                    } else {
                        revision
                            .as_u64()
                            .filter(|number| *number <= 9_007_199_254_740_991)
                            .ok_or_else(|| {
                                sailry_protocol::Fault::invalid(
                                    "revision must be an exact integer or decimal string",
                                )
                            })?
                    };
                    *revision = json!(parsed);
                }
                let command: Command = serde_json::from_value(value)
                    .map_err(|_| sailry_protocol::Fault::invalid("invalid SDK command"))?;
                match &command {
                    Command::ReadInstallations { requests } => {
                        sailry_protocol::installation::validate_requests(requests)?;
                        if requests.iter().any(|id| !self.installations.contains(id)) {
                            return Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "installation request is not owned by this callback",
                            ));
                        }
                    }
                    Command::DismissInstallation { request }
                        if !self.installations.contains(request) =>
                    {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "installation request is not owned by this callback",
                        ));
                    }
                    _ => {}
                }
                if command.durable()
                    && let Some(execution) = &self.execution
                {
                    let execution = execution.lock().map_err(|_| {
                        sailry_protocol::Fault::invalid("tool execution state is unavailable")
                    })?;
                    if execution.mode == sailry_protocol::WorkMode::Plan {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "planning turns cannot dispatch durable commands",
                        ));
                    }
                    if execution.read_only {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "read-only script tools cannot dispatch durable commands",
                        ));
                    }
                }
                if self.execution.is_some()
                    && command.durable()
                    && !matches!(
                        command,
                        Command::WritePluginValue { .. }
                            | Command::RemovePluginValue { .. }
                            | Command::WritePluginConversationValue { .. }
                            | Command::RemovePluginConversationValue { .. }
                            | Command::RegisterLocalModel { .. }
                            | Command::InstallLocalModel { .. }
                    )
                    && !matches!(&command, Command::PluginTransaction { operations }
                        if operations.iter().all(|operation| matches!(operation,
                            plugin::transaction::Operation::Write { .. }
                                | plugin::transaction::Operation::Remove { .. }
                                | plugin::transaction::Operation::ConversationWrite { .. }
                                | plugin::transaction::Operation::ConversationRemove { .. }
                                | plugin::transaction::Operation::QueuePause { .. }
                                | plugin::transaction::Operation::Continue { .. }
                                | plugin::transaction::Operation::Stop { .. })))
                {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "script tools cannot dispatch durable commands",
                    ));
                }
                if !allowed(&command) {
                    return Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "command is not exposed to host callbacks",
                    ));
                }
                let mut request =
                    Request::new(self.target, command).with_plugin(self.context.clone());
                if request.command.durable()
                    && let Some(execution) = &self.execution
                {
                    let call = execution
                        .lock()
                        .map_err(|_| {
                            sailry_protocol::Fault::invalid("tool execution state is unavailable")
                        })?
                        .call_id
                        .ok_or_else(|| {
                            sailry_protocol::Fault::invalid(
                                "durable writes require a tool identity",
                            )
                        })?;
                    let mut identity =
                        blake3::Hasher::new_derive_key("Sailry plugin tool write v1");
                    identity.update(call.to_string().as_bytes());
                    identity.update(&self.writes.to_le_bytes());
                    let mut bytes: [u8; 16] =
                        identity.finalize().as_bytes()[..16].try_into().unwrap();
                    bytes[6] = (bytes[6] & 0x0f) | 0x80;
                    bytes[8] = (bytes[8] & 0x3f) | 0x80;
                    request.id =
                        sailry_protocol::RequestId::try_from(uuid::Uuid::from_bytes(bytes))
                            .map_err(|_| {
                                sailry_protocol::Fault::invalid("invalid tool write identity")
                            })?;
                    self.writes += 1;
                }
                if matches!(operation, "read" | "models.list") {
                    if request.command.durable() {
                        return Err(sailry_protocol::Fault::invalid(
                            "SDK reads cannot execute writes",
                        ));
                    }
                    return self.dispatch(request).map(output);
                }
                if self.drafts.len() >= 64 {
                    return Err(Fault::new(
                        ErrorCode::Busy,
                        "plugin request drafts are full",
                    ));
                }
                let id = request.id.to_string();
                self.drafts.insert(id.clone(), request);
                Ok(json!(id))
            }
            "complete" => {
                let id: String = serde_json::from_str(body)
                    .map_err(|_| sailry_protocol::Fault::invalid("invalid request ID"))?;
                let request = self
                    .drafts
                    .get(&id)
                    .ok_or_else(|| {
                        sailry_protocol::Fault::invalid("plugin request draft is unavailable")
                    })?
                    .clone();
                // Business faults remain explicit outcomes, matching the desktop SDK.
                if matches!(request.command, Command::InstallLocalModel { .. }) {
                    if !self.installations.contains(&request.id)
                        && self.installations.len() >= sailry_protocol::installation::MAX_REQUESTS
                    {
                        return Err(Fault::new(
                            ErrorCode::Busy,
                            "callback installation observations are full",
                        ));
                    }
                    self.installations.insert(request.id);
                }
                Ok(match self.dispatch(request) {
                    Ok(result) => json!({"Ok": output(result)}),
                    Err(error) => json!({"Err": error}),
                })
            }
            "forget" => {
                let id: String = serde_json::from_str(body)
                    .map_err(|_| sailry_protocol::Fault::invalid("invalid request ID"))?;
                self.drafts.remove(&id);
                Ok(Json::Null)
            }
            _ => Err(sailry_protocol::Fault::invalid(
                "unknown host SDK operation",
            )),
        }
    }

    fn dispatch(&mut self, request: Request) -> sailry_link::Response {
        let result = (self.dispatch)(request);
        if (self.control.stop.is_cancelled() || Instant::now() >= self.control.deadline)
            && let Err(fault) = &result
        {
            *self
                .control
                .failure
                .lock()
                .expect("script result lock is valid") = Some(fault.clone());
        }
        result
    }
}

fn output(value: Output) -> Json {
    if let Output::Roles(roles) = value {
        return json!({"kind":"roles","data":roles.into_iter().map(|role| {
            let mut value = json!(role);
            value["revision"] = json!(role.revision.to_string());
            value
        }).collect::<Vec<_>>()});
    }
    plugin::transaction::sdk::output(value)
}

fn allowed(command: &Command) -> bool {
    matches!(
        command,
        Command::RequestProviderHttp { .. }
            | Command::ListConnectorTypes
            | Command::ListConnections
            | Command::StartConnectorLogin { .. }
            | Command::CancelConnectorLogin { .. }
            | Command::ExecuteProviderHttp { .. }
            | Command::PluginTransaction { .. }
            | Command::ListRoles
            | Command::ReadArtifact { .. }
            | Command::ListArtifacts { .. }
            | Command::ListArtifactRevisions { .. }
            | Command::ReadSession { .. }
            | Command::ReadConversation { .. }
            | Command::ReadTurnCapture { .. }
            | Command::ReadTurn { .. }
            | Command::SubmitTurn { .. }
            | Command::ContinueTurn { .. }
            | Command::StopTurn { .. }
            | Command::StartSession(_)
            | Command::ReadProjectCatalog
            | Command::ResolvePluginModel { .. }
            | Command::SearchModels(_)
            | Command::ReadModelVersions { .. }
            | Command::ResolveModel { .. }
            | Command::ReadLocalModels { .. }
            | Command::Decide(_)
            | Command::RegisterLocalModel { .. }
            | Command::InstallLocalModel { .. }
            | Command::ReadInstallations { .. }
            | Command::DismissInstallation { .. }
            | Command::ReadPluginValue { .. }
            | Command::ReadPluginConversationValue { .. }
            | Command::ListPluginKeys { .. }
            | Command::WritePluginValue { .. }
            | Command::WritePluginConversationValue { .. }
            | Command::WriteIndexedPluginValue { .. }
            | Command::SearchPluginValues(_)
            | Command::FetchWeb(_)
            | Command::CancelWebFetch { .. }
            | Command::QueryKnowledge(_)
            | Command::ListKnowledgeCollections
            | Command::CreateKnowledgeCollection { .. }
            | Command::RebuildKnowledgeCollection { .. }
            | Command::DeleteKnowledgeCollection { .. }
            | Command::RemovePluginValue { .. }
            | Command::RemovePluginConversationValue { .. }
            | Command::ReadPluginSettings { .. }
            | Command::ReadPluginConnection { .. }
            | Command::PublishNotification { .. }
            | Command::Dispatch { .. }
            | Command::StopDispatchTurn { .. }
            | Command::ReadFile { .. }
            | Command::ReadFileLines { .. }
            | Command::FindFiles { .. }
            | Command::WriteFile { .. }
            | Command::EditFile { .. }
            | Command::DeleteFile { .. }
            | Command::MoveFile { .. }
            | Command::CreateDirectory { .. }
            | Command::ListDirectory { .. }
            | Command::SearchFiles { .. }
            | Command::InspectGit { .. }
            | Command::ReadGitDiff { .. }
            | Command::ReadGitLog { .. }
            | Command::ReadGitShow { .. }
            | Command::ReadGitBlame { .. }
            | Command::ListGitChanges { .. }
            | Command::ReadGitChange { .. }
            | Command::ReadGitContext { .. }
            | Command::ReadGitCommit { .. }
            | Command::ListGitBranches { .. }
            | Command::ResolveGitRevision { .. }
    )
}
