use super::*;
use rquickjs::{Ctx, prelude::MutFn};
use sailry_protocol::{Command, NodeId, Output, Request};
use serde_json::{Value as Json, json};

pub(super) fn install(
    ctx: &Ctx<'_>,
    target: NodeId,
    context: plugin::Context,
    stop: CancellationToken,
    deadline: Instant,
    dispatch: impl FnMut(Request) -> sailry_link::Response + 'static,
    execution: Option<std::sync::Arc<std::sync::Mutex<execution::State>>>,
) -> rquickjs::Result<()> {
    let mut bridge = Bridge {
        execution,
        target,
        context,
        stop,
        deadline,
        dispatch,
        drafts: BTreeMap::new(),
        calls: 0,
        writes: 0,
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
    stop: CancellationToken,
    deadline: Instant,
    dispatch: F,
    drafts: BTreeMap<String, Request>,
    calls: usize,
    writes: u64,
}

impl<F: FnMut(Request) -> sailry_link::Response> Bridge<F> {
    fn call(&mut self, operation: &str, body: &str) -> Result<Json, Fault> {
        if self.stop.is_cancelled() || Instant::now() >= self.deadline {
            return Err(Fault::new(ErrorCode::Cancelled, "plugin callback stopped"));
        }
        if body.len() > plugin::host::MAX_DATA_BYTES || self.calls >= 1024 {
            return Err(invalid("plugin callback SDK budget exhausted"));
        }
        self.calls += 1;
        match operation {
            "id.new" => Ok(json!(sailry_protocol::RequestId::new())),
            "id.call" | "turn.read" | "turn.stage" => {
                let execution = self
                    .execution
                    .as_ref()
                    .ok_or_else(|| invalid("operation requires an Agent tool invocation"))?;
                let mut execution = execution
                    .lock()
                    .map_err(|_| invalid("tool execution state is unavailable"))?;
                execution.call(operation, body)
            }
            "context" => {
                let mut value = json!(self.context);
                value["package"]["settings_revision"] =
                    json!(self.context.package.settings_revision.to_string());
                Ok(value)
            }
            "prepare" | "read" => {
                let mut value: Json =
                    serde_json::from_str(body).map_err(|_| invalid("invalid SDK command"))?;
                if let Some(revision) = value.pointer_mut("/data/package/settings_revision") {
                    let text = revision
                        .as_str()
                        .ok_or_else(|| invalid("package revision must be a decimal string"))?;
                    let parsed = text
                        .parse::<u64>()
                        .map_err(|_| invalid("invalid package revision"))?;
                    if parsed.to_string() != text {
                        return Err(invalid("invalid package revision"));
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
                    )
                ) && let Some(revision) = value.pointer_mut("/data/expected_revision")
                {
                    let parsed = if let Some(text) = revision.as_str() {
                        text.parse::<u64>()
                            .map_err(|_| invalid("invalid revision"))?
                    } else {
                        revision
                            .as_u64()
                            .filter(|number| *number <= 9_007_199_254_740_991)
                            .ok_or_else(|| {
                                invalid("revision must be an exact integer or decimal string")
                            })?
                    };
                    *revision = json!(parsed);
                }
                let command: Command =
                    serde_json::from_value(value).map_err(|_| invalid("invalid SDK command"))?;
                if command.durable()
                    && let Some(execution) = &self.execution
                {
                    let execution = execution
                        .lock()
                        .map_err(|_| invalid("tool execution state is unavailable"))?;
                    if execution.mode == sailry_protocol::WorkMode::Plan {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "planning turns cannot write plugin storage",
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
                    )
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
                        .map_err(|_| invalid("tool execution state is unavailable"))?
                        .call_id
                        .ok_or_else(|| invalid("durable writes require a tool identity"))?;
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
                            .map_err(|_| invalid("invalid tool write identity"))?;
                    self.writes += 1;
                }
                if operation == "read" {
                    if request.command.durable() {
                        return Err(invalid("SDK reads cannot execute writes"));
                    }
                    return (self.dispatch)(request).map(output);
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
                let id: String =
                    serde_json::from_str(body).map_err(|_| invalid("invalid request ID"))?;
                let request = self
                    .drafts
                    .get(&id)
                    .ok_or_else(|| invalid("plugin request draft is unavailable"))?
                    .clone();
                // Business faults remain explicit outcomes, matching the desktop SDK.
                Ok(match (self.dispatch)(request) {
                    Ok(result) => json!({"Ok": output(result)}),
                    Err(error) => json!({"Err": error}),
                })
            }
            "forget" => {
                let id: String =
                    serde_json::from_str(body).map_err(|_| invalid("invalid request ID"))?;
                self.drafts.remove(&id);
                Ok(Json::Null)
            }
            _ => Err(invalid("unknown host SDK operation")),
        }
    }
}

fn output(value: Output) -> Json {
    plugin::transaction::sdk::output(value)
}

fn allowed(command: &Command) -> bool {
    matches!(
        command,
        Command::PluginTransaction { .. }
            | Command::ReadSession { .. }
            | Command::ReadConversation { .. }
            | Command::ReadTurn { .. }
            | Command::SubmitTurn { .. }
            | Command::ContinueTurn { .. }
            | Command::StopTurn { .. }
            | Command::StartSession(_)
            | Command::ReadProjectCatalog
            | Command::ResolvePluginModel { .. }
            | Command::ReadPluginValue { .. }
            | Command::ReadPluginConversationValue { .. }
            | Command::ListPluginKeys { .. }
            | Command::WritePluginValue { .. }
            | Command::WritePluginConversationValue { .. }
            | Command::WriteIndexedPluginValue { .. }
            | Command::SearchPluginValues(_)
            | Command::RemovePluginValue { .. }
            | Command::RemovePluginConversationValue { .. }
            | Command::ReadPluginSettings { .. }
            | Command::PublishNotification { .. }
            | Command::Dispatch { .. }
            | Command::StopDispatchTurn { .. }
            | Command::ReadFile { .. }
            | Command::WriteFile { .. }
            | Command::ListDirectory { .. }
            | Command::SearchFiles { .. }
            | Command::InspectGit { .. }
            | Command::ReadGitDiff { .. }
            | Command::ReadGitLog { .. }
            | Command::ReadGitCommit { .. }
            | Command::ListGitBranches { .. }
            | Command::ResolveGitRevision { .. }
    )
}
