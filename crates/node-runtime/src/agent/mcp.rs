//! Namespaced ADK MCP tools bound to the original turn, approval and package snapshot.
mod input;
mod limits;
mod pool;
pub(crate) use pool::Pool;
mod schema;
mod transport;

use super::catalog::Registration;
use super::*;
use adk_core::{Tool, ToolConfirmationRequest, ToolContext};
use async_trait::async_trait;
use rmcp::{
    RoleClient,
    model::{ClientInfo, Tool as Descriptor},
    service::RunningService,
};
use sailry_protocol::plugin::{
    Reference,
    conversation::{Declaration, Tool as SelectedTool},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

type Service = RunningService<RoleClient, Arc<input::Handler>>;
const MAX_TOOLS: usize = 64;

#[derive(Default)]
pub(super) struct Connections {
    active: Vec<Connected>,
    poisoned: Arc<AtomicBool>,
    failure: Option<Fault>,
}

struct Connected {
    reference: sailry_protocol::plugin::Reference,
    name: String,
    transport: transport::Connection,
    peer: rmcp::Peer<RoleClient>,
    toolset: adk_tool::McpToolset<Arc<input::Handler>>,
    handler: Arc<input::Handler>,
}

impl Connected {
    async fn close(mut self) -> Result<(), Fault> {
        drop(self.toolset);
        self.transport.close().await
    }
}

impl Connections {
    pub(super) fn reusable(&self) -> bool {
        self.failure.is_none() && !self.poisoned.load(Ordering::Relaxed)
    }
    pub(super) async fn bind(
        &mut self,
        resources: &crate::plugins::resources::Resources,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        stop: &CancellationToken,
        assistant: Option<(&Reference, &Declaration)>,
    ) -> (Vec<Registration>, String) {
        let mut tools: Vec<Registration> = Vec::new();
        let mut diagnostics = String::new();
        let mut previous = std::mem::take(&mut self.active);
        let configurations = resources.mcp().await;
        let selected = configurations.into_iter().filter(|config| {
            assistant.is_none_or(|(package, declaration)| {
                config.reference == *package
                    && declaration.tools.iter().any(|tool| {
                        matches!(tool, SelectedTool::Plugin { server: Some(server), .. } if server == &config.name)
                    })
            })
        });
        for (index, config) in selected.enumerate() {
            if stop.is_cancelled() {
                break;
            }
            let result = async {
                if index >= crate::plugins::mcp::MAX_SERVERS { return Err(unavailable()); }
                let launch = config.launch?;
                let authorized = tokio::select! {
                    _ = stop.cancelled() => return Err(unavailable()),
                    result = tokio::time::timeout(Duration::from_secs(10), resources.mcp_client(ingress, &config.plugin, &config.name, &launch)) => result.map_err(|_| unavailable())??,
                };
                let cached = previous.iter().position(|connection| connection.reference == config.reference && connection.name == config.name)
                    .map(|index| previous.remove(index));
                let cached = match cached {
                    Some(connection) if connection.toolset.is_closed().await => {
                        connection.close().await?;
                        None
                    }
                    connection => connection,
                };
                let connection = if let Some(connection) = cached {
                    connection
                } else {
                    let handler = Arc::new(input::Handler::new());
                    let (transport, service) = transport::Connection::open(launch, authorized, stop, handler.clone()).await?;
                    let peer = service.peer().clone();
                    let toolset = adk_tool::McpToolset::new(service)
                        .with_task_support(adk_tool::mcp::McpTaskConfig::enabled().no_timeout());
                    Connected { reference: config.reference, name: config.name.clone(), transport, peer, toolset, handler }
                };
                let listed = tokio::select! {
                    _ = stop.cancelled() => Err(unavailable()),
                    result = tokio::time::timeout(Duration::from_secs(10), list(&connection.peer)) => result.unwrap_or_else(|_| Err(unavailable())),
                };
                match listed {
                    Ok(descriptors) if tools.len() + descriptors.len() <= MAX_TOOLS => {
                        for descriptor in descriptors {
                            let presentation = resources.tool_presentation(&config.plugin, &config.name, &descriptor.name);
                            let grouping = resources.tool_grouping(&config.plugin, &config.name, &descriptor.name);
                            let display = resources.tool_display(&config.plugin, &config.name, &descriptor.name);
                            let mut registration = Registration::new(Arc::new(Bound {
                                name: alias(&config.plugin, &config.name, &descriptor.name),
                                description: format!("MCP {}/{}/{}\n{}", config.plugin, config.name, descriptor.name, descriptor.description.as_deref().unwrap_or("")),
                                descriptor, toolset: connection.toolset.clone(), ingress: ingress.clone(), handler: connection.handler.clone(), poisoned: self.poisoned.clone(),
                                turn: invocation.turn.id, session: invocation.turn.session.to_string(), stop: stop.clone(),
                            }), presentation);
                            registration.grouping = grouping;
                            registration.display = display.map(|display| (registration.tool.name().into(), display));
                            registration.plugin = Some(config.plugin.clone());
                            tools.push(registration);
                        }
                        self.active.push(connection);
                        Ok(())
                    }
                    _ => {
                        connection.close().await?;
                        Err(unavailable())
                    }
                }
            }.await;
            if let Err(error) = result {
                if error.code == ErrorCode::OutcomeUnknown {
                    self.failure = Some(error);
                }
                // Do not put endpoints, environment values or SDK errors in model input.
                if diagnostics.len() < 16 * 1024 {
                    diagnostics.push_str(&format!(
                        "\nMCP server {}/{} is unavailable; do not claim to use it.",
                        config.plugin, config.name
                    ));
                }
            }
        }
        for result in futures::future::join_all(previous.into_iter().map(Connected::close)).await {
            if let Err(error) = result {
                self.failure = Some(error);
            }
        }
        (tools, diagnostics)
    }

    pub(super) async fn settle(&mut self) -> Result<(), Fault> {
        let mut failure = self.failure.take();
        for connection in &self.active {
            if !matches!(
                tokio::time::timeout(
                    Duration::from_secs(5),
                    connection.toolset.cancel_pending_tasks()
                )
                .await,
                Ok(Ok(()))
            ) {
                failure = Some(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "MCP task termination could not be confirmed",
                ));
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub(super) async fn release(&mut self) -> Result<(), Fault> {
        let active = std::mem::take(&mut self.active);
        futures::future::join_all(active.into_iter().map(Connected::close))
            .await
            .into_iter()
            .collect()
    }

    pub(super) async fn close(&mut self) -> Result<(), Fault> {
        let settled = self.settle().await;
        let released = self.release().await;
        settled.and(released)
    }
}

async fn list(service: &rmcp::Peer<RoleClient>) -> Result<Vec<Descriptor>, Fault> {
    let mut tools = Vec::new();
    let mut names = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut cursor = None;
    let mut bytes = 0;
    loop {
        let page = service
            .list_tools(cursor.map(|cursor| {
                let mut params = rmcp::model::PaginatedRequestParams::default();
                params.cursor = Some(cursor);
                params
            }))
            .await
            .map_err(|_| unavailable())?;
        for tool in page.tools {
            bytes += serde_json::to_vec(&tool).map_err(|_| unavailable())?.len();
            if tools.len() >= MAX_TOOLS
                || bytes > 256 * 1024
                || tool.name.is_empty()
                || !names.insert(tool.name.to_string())
            {
                return Err(unavailable());
            }
            tools.push(tool);
        }
        cursor = page.next_cursor;
        match &cursor {
            None => return Ok(tools),
            Some(cursor) if cursors.len() >= MAX_TOOLS || !cursors.insert(cursor.clone()) => {
                return Err(unavailable());
            }
            Some(_) => {}
        }
    }
}

pub(super) fn alias(plugin: &str, server: &str, tool: &str) -> String {
    let mut hash = blake3::Hasher::new_derive_key("Sailry MCP tool name v1");
    for part in [plugin, server, tool] {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let label: String = format!("{plugin}_{server}_{tool}")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    format!("mcp_{label}_{}", &hash.finalize().to_hex()[..24])
}

struct Bound {
    name: String,
    description: String,
    descriptor: Descriptor,
    toolset: adk_tool::McpToolset<Arc<input::Handler>>,
    handler: Arc<input::Handler>,
    ingress: Arc<Ingress>,
    turn: TurnId,
    session: String,
    stop: CancellationToken,
    poisoned: Arc<AtomicBool>,
}

#[async_trait]
impl Tool for Bound {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(Value::Object((*self.descriptor.input_schema).clone()))
    }
    fn response_schema(&self) -> Option<Value> {
        self.descriptor
            .output_schema
            .as_ref()
            .map(|schema| Value::Object((**schema).clone()))
    }
    fn is_read_only(&self) -> bool {
        self.descriptor
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            .unwrap_or(false)
    }
    fn is_concurrency_safe(&self) -> bool {
        self.is_read_only()
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
            return Err(adk_core::AdkError::tool(
                "MCP invocation is unavailable or cancelled",
            ));
        }
        let Value::Object(args) = &arguments else {
            return Err(adk_core::AdkError::tool("MCP arguments must be an object"));
        };
        if !self.is_read_only() {
            self.ingress
                .authorize(
                    self.turn,
                    ToolConfirmationRequest {
                        tool_name: self.name.clone(),
                        function_call_id: Some(context.function_call_id().into()),
                        args: arguments.clone(),
                    },
                )
                .await
                .map_err(|_| adk_core::AdkError::tool("MCP authorization is unavailable"))?;
        }
        // Neither ADK tool-call retries nor HTTP session-expiry replay is enabled.
        let _serial = tokio::select! {
            _ = self.stop.cancelled() => return Err(adk_core::AdkError::tool("MCP invocation was cancelled")),
            guard = self.handler.serial.lock() => guard,
        };
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(adk_core::AdkError::tool("MCP invocation was cancelled"));
        }
        let active = self.handler.begin(
            self.ingress.clone(),
            self.turn,
            self.session.clone(),
            ToolConfirmationRequest {
                tool_name: self.name.clone(),
                function_call_id: Some(context.function_call_id().into()),
                args: arguments.clone(),
            },
            &self.stop,
        );
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(adk_core::AdkError::tool("MCP invocation was cancelled; remote effects may be uncertain")),
            result = active.run(self.toolset.call_tool_value(&self.descriptor.name, args.clone())) => {
                if !matches!(&result, Ok(Ok(_))) { self.poisoned.store(true, Ordering::Relaxed); }
                match result {
                    Ok(Ok(value)) => Ok(value),
                    Ok(Err(error)) => Ok(json!({"error": {"code": "outcome_unknown", "message": format!("MCP call failed and was not replayed; side effects may be uncertain: {error}")}})),
                    _ => Ok(json!({"error": {"code": "outcome_unknown", "message": "MCP result is unavailable; the call was not replayed and side effects may be uncertain"}})),
                }
            }
        }
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "MCP server is unavailable")
}

#[cfg(test)]
mod names {
    use super::*;

    #[test]
    fn bounds_tool_aliases() {
        let tool = "Unicode 工具 🙂".repeat(20);
        let name = alias("example", "service", &tool);
        assert!(name.len() <= 64);
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        );
        assert_eq!(name, alias("example", "service", &tool));
        assert_ne!(
            alias("example", "a.b", "read"),
            alias("example", "a_b", "read")
        );
        assert_ne!(
            alias("example", "service", "read"),
            alias("other", "service", "read")
        );
    }
}
