//! Namespaced MCP tools retain the original turn, approval and package snapshot.
pub(super) mod check;
mod diagnostics;
mod input;
mod limits;
pub(super) mod management;
mod pool;
pub(crate) use pool::Pool;
mod remote;
mod schema;
mod transport;

use super::catalog::Registration;
use super::*;
use crate::agent::{canonical::ToolIdentity, catalog::CallContext};
use async_trait::async_trait;
use goose_agent::{operation::Emitter, tool::ToolProvider};
use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData};
use rmcp::{
    RoleClient,
    model::{ClientConfig, Tool as Descriptor},
    service::RunningService,
};
use sailry_protocol::{
    RequestId,
    plugin::{
        Reference,
        conversation::{Declaration, Tool as SelectedTool},
    },
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
    images: Vec<(Vec<String>, usize)>,
}

struct Connected {
    reference: sailry_protocol::plugin::Reference,
    name: String,
    transport: transport::Shared,
    peer: rmcp::Peer<RoleClient>,
    toolset: Arc<remote::Remote>,
    handler: Arc<input::Handler>,
    connector: Option<crate::plugins::connectors::mcp::Authorization>,
    python: Option<crate::python::Ready>,
    reuse: bool,
}

impl Connected {
    async fn close(self) -> Result<(), Fault> {
        drop(self.toolset);
        self.transport.lock().await.close().await
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
        self.images.clear();
        let mut tools: Vec<Registration> = Vec::new();
        let mut diagnostics = String::new();
        let mut previous = std::mem::take(&mut self.active);
        let configurations = resources.mcp(&ingress.environment).await;
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
                let mut launch = config.launch?;
                let context=sailry_protocol::plugin::Context{package:config.reference.clone(),worktree:Some(invocation.worktree),session:Some(invocation.turn.session),turn:Some(invocation.turn.id),invocation:None,surface:Default::default()};
                let reuse = config.options.reuse;
                let exclusive = config.options.exclusive.then(|| ingress.agents.mcp.service(&config.plugin, &config.name));
                let python = match &launch { crate::plugins::resources::mcp::Launch::Stdio {python,..} => python.clone(), _ => None };
                let connector = tokio::select! {
                    _ = stop.cancelled() => return Err(unavailable()),
                    result = tokio::time::timeout(Duration::from_secs(10), resources.mcp_authorization(ingress, context, &config.name, &mut launch)) => result.map_err(|_| unavailable())??,
                };
                let cached = previous.iter().position(|connection| connection.reference == config.reference && connection.name == config.name)
                    .map(|index| previous.remove(index));
                let cached = match cached {
                    Some(connection) if connection.toolset.is_closed() || connection.connector.as_ref().map(|value|value.key()) != connector.as_ref().map(|value|value.key()) => {
                        connection.close().await?;
                        None
                    }
                    connection => connection,
                };
                let connection = if let Some(connection) = cached {
                    connection
                } else {
                    let handler = Arc::new(input::Handler::new());
                    let (mut transport, service) = transport::Connection::open(launch, connector.clone(), stop, handler.clone(), !reuse, &ingress.environment).await?;
                    let peer = service.peer().clone();
                    let toolset = remote::Remote::new(service);
                    transport.track_tasks(toolset.clone());
                    Connected { reference: config.reference, name: config.name.clone(), transport: Arc::new(tokio::sync::Mutex::new(transport)), peer, toolset, handler, connector, python, reuse }
                };
                let listed = tokio::select! {
                    _ = stop.cancelled() => Err(unavailable()),
                    result = tokio::time::timeout(Duration::from_secs(10), list(&connection.peer)) => result.unwrap_or_else(|_| Err(unavailable())),
                };
                match listed {
                    Ok(descriptors) if tools.len() + descriptors.len() <= MAX_TOOLS => {
                        let closed = connection.transport.lock().await.stopped();
                        if let Some(keep) = config.options.keep_images {
                            self.images.push((descriptors.iter().map(|descriptor| alias(&config.plugin, &config.name, &descriptor.name)).collect(), keep));
                        }
                        for descriptor in descriptors {
                            let presentation = resources.tool_presentation(&config.plugin, &config.name, &descriptor.name);
                            let grouping = resources.tool_grouping(&config.plugin, &config.name, &descriptor.name);
                            let display = resources.tool_display(&config.plugin, &config.name, &descriptor.name).or_else(|| {
                                resources.tool_declaration(&config.plugin, &config.name, &descriptor.name).is_none().then(|| {
                                    let mut navigation = sailry_protocol::plugin::desktop::Navigation {
                                        label: descriptor.name.to_string(), icon: None, locales: Default::default(),
                                    };
                                    if !navigation.valid() {
                                        navigation.label = alias(&config.plugin, &config.name, &descriptor.name);
                                    }
                                    let mut display = sailry_protocol::tool::Display::from(navigation);
                                    display.raw_result = true;
                                    display
                                })
                            });
                            let bound = Arc::new(Bound {
                                name: alias(&config.plugin, &config.name, &descriptor.name),
                                description: format!("MCP {}/{}/{}\n{}", config.plugin, config.name, descriptor.name, descriptor.description.as_deref().unwrap_or("")),
                                descriptor, package: connection.reference.clone(), toolset: connection.toolset.clone(), ingress: ingress.clone(), handler: connection.handler.clone(), poisoned: self.poisoned.clone(),
                                turn: invocation.turn.id, session: invocation.turn.session.to_string(), stop: stop.clone(),
                                connector: connection.connector.clone(),
                                python: connection.python.clone(),
                                exclusive: exclusive.clone(),
                                transport: connection.transport.clone(),
                                closed: closed.clone(),
                            });
                            let mut registration = Registration::new(bound.declaration(), bound.clone(), presentation);
                            registration.grouping = grouping;
                            registration.display = display.map(|display| (registration.tool.name.to_string(), display));
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

    pub(super) fn screenshots(&self) -> Vec<(Vec<String>, usize)> {
        self.images.clone()
    }

    pub(super) async fn settle(&mut self) -> Result<(), Fault> {
        let mut failure = self.failure.take();
        let mut retained = Vec::new();
        for connection in std::mem::take(&mut self.active) {
            if connection.reuse {
                if let Err(error) = connection.transport.lock().await.settle().await {
                    failure = Some(error);
                }
                retained.push(connection);
            } else if let Err(error) = connection.close().await {
                failure = Some(error);
            }
        }
        self.active = retained;
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
        let failure = self.failure.take();
        let released = self.release().await;
        failure.map_or(released, Err)
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
    package: sailry_protocol::plugin::Reference,
    name: String,
    description: String,
    descriptor: Descriptor,
    toolset: Arc<remote::Remote>,
    handler: Arc<input::Handler>,
    ingress: Arc<Ingress>,
    turn: TurnId,
    session: String,
    stop: CancellationToken,
    poisoned: Arc<AtomicBool>,
    connector: Option<crate::plugins::connectors::mcp::Authorization>,
    python: Option<crate::python::Ready>,
    exclusive: Option<Arc<tokio::sync::Mutex<()>>>,
    transport: transport::Shared,
    closed: CancellationToken,
}

impl Bound {
    fn name(&self) -> &str {
        &self.name
    }
    fn declaration(&self) -> Descriptor {
        let mut tool = self.descriptor.clone();
        tool.name = self.name.clone().into();
        tool.description = Some(self.description.clone().into());
        tool
    }
    fn is_read_only(&self) -> bool {
        self.descriptor
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.read_only_hint)
            .unwrap_or(false)
    }
    async fn execute(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
    ) -> Result<CallToolResult, Fault> {
        let identity = crate::logging::Context {
            node: Some(self.ingress.node),
            turn: Some(self.turn),
            session: self.session.parse().ok(),
            package: Some(self.package.clone()),
            handler: Some(self.descriptor.name.to_string()),
            call: Some(RequestId::new()),
            ..Default::default()
        };
        let mut span = diagnostics::begin(identity, "call", &self.stop);
        let result = self.call(context.clone(), arguments, &mut span).await;
        if let Ok(value) = &result
            && (value.is_error.unwrap_or(false))
            && let Some(timeline) = self.ingress.agents.timeline(self.turn)
        {
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                self.name(),
                None,
            );
        }
        diagnostics::finish(
            &mut span,
            &result
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap())
                .map_err(Clone::clone),
            self.stop.is_cancelled() || context.is_cancelled(),
        );
        result
    }
}

impl Bound {
    async fn call(
        &self,
        context: Arc<CallContext>,
        arguments: Value,
        span: &mut crate::logging::Span,
    ) -> Result<CallToolResult, Fault> {
        if context.session_id() != self.session
            || context.is_cancelled()
            || self.stop.is_cancelled()
            || self.closed.is_cancelled()
        {
            return Err(Fault::invalid("MCP invocation is unavailable or cancelled"));
        }
        let Value::Object(args) = &arguments else {
            return Err(Fault::invalid("MCP arguments must be an object"));
        };
        if let Some(ready) = &self.python {
            diagnostics::stage(
                span,
                &self.stop,
                "preparation",
                ErrorCode::Unavailable,
                async {
                    let manager = self
                        .ingress
                        .plugins
                        .python(&self.ingress.environment)
                        .await
                        .map_err(|error| Fault::invalid(error.to_string()))?;
                    manager
                        .refresh(ready, &self.stop)
                        .await
                        .map_err(|error| Fault::invalid(error.to_string()))
                },
            )
            .await?;
        }
        if !self.is_read_only() {
            diagnostics::stage(
                span,
                &self.stop,
                "authorization",
                ErrorCode::PermissionDenied,
                self.ingress.authorize(self.turn, context.request(), None),
            )
            .await
            .map_err(|_| Fault::invalid("MCP authorization is unavailable"))?;
        }
        // Installed names are unique in this Node/profile. Content and settings
        // revisions deliberately do not split a service across admitted turns.
        let waiting = async {
            let service = if !self.is_read_only() {
                if let Some(lock) = &self.exclusive {
                    Some(tokio::select! {
                        biased;
                        _ = self.stop.cancelled() => return Err(Fault::invalid("MCP invocation was cancelled")),
                        _ = self.closed.cancelled() => return Err(Fault::invalid("MCP connection is closed")),
                        guard = lock.clone().lock_owned() => guard,
                    })
                } else {
                    None
                }
            } else {
                None
            };
            // Remote calls are not replayed after an uncertain result.
            let serial = tokio::select! {
                _ = self.stop.cancelled() => return Err(Fault::invalid("MCP invocation was cancelled")),
                _ = self.closed.cancelled() => return Err(Fault::invalid("MCP connection is closed")),
                guard = self.handler.serial.lock() => guard,
            };
            Ok::<_, Fault>((service, serial))
        };
        let (service, _serial) = diagnostics::stage(
            span,
            &self.stop,
            "lock_wait",
            ErrorCode::Unavailable,
            waiting,
        )
        .await?;
        if context.is_cancelled() || self.stop.is_cancelled() || self.closed.is_cancelled() {
            return Err(Fault::invalid("MCP invocation was cancelled"));
        }
        if let Some(connector) = &self.connector
            && diagnostics::stage(
                span,
                &self.stop,
                "connection_authorization",
                ErrorCode::PermissionDenied,
                connector.validate(),
            )
            .await
            .is_err()
        {
            self.poisoned.store(true, Ordering::Relaxed);
            return Err(Fault::invalid("MCP connection is unavailable or changed"));
        }
        diagnostics::external(span);
        let mut external =
            diagnostics::begin(span.entry.context.clone(), "external_roundtrip", &self.stop);
        diagnostics::external(&mut external);
        let active = self.handler.begin(
            self.ingress.clone(),
            self.turn,
            self.session.clone(),
            context.request(),
            &self.stop,
        );
        let mut call = service.map(|guard| {
            transport::Call::new(
                self.transport.clone(),
                self.closed.clone(),
                guard,
                self.poisoned.clone(),
            )
        });
        let result = tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::invalid("MCP invocation was cancelled; remote effects may be uncertain")),
            result = active.run(self.toolset.call(&self.descriptor.name, args.clone())) => {
                if matches!(&result, Ok(Ok(_))) {
                    if let Some(call) = &mut call { call.confirmed(); }
                } else { self.poisoned.store(true, Ordering::Relaxed); }
                match result {
                    Ok(Ok(value)) => {
                        if let Some(connector) = &self.connector {
                            let mut json = serde_json::to_value(value).map_err(|_| Fault::invalid("MCP result could not be encoded"))?;
                            connector.redact(&mut json);
                            serde_json::from_value(json).map_err(|_| Fault::invalid("MCP result could not be decoded"))
                        } else { Ok(value) }
                    },
                    Ok(Err(error)) => {
                        let mut value = serde_json::to_value(error).unwrap();
                        if let Some(connector) = &self.connector { connector.redact(&mut value); }
                        let error: Fault = serde_json::from_value(value).unwrap();
                        Err(Fault::new(ErrorCode::OutcomeUnknown, format!("MCP call failed without replay; remote effects may be uncertain: {}", error.message)))
                    },
                    Err(_) => Err(Fault::new(ErrorCode::OutcomeUnknown, "MCP result is unavailable; remote effects may be uncertain")),
                }
            }
        };
        diagnostics::finish(
            &mut external,
            &result
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap())
                .map_err(Clone::clone),
            self.stop.is_cancelled(),
        );
        result
    }
}

#[async_trait]
impl ToolProvider<CallContext> for Bound {
    async fn tools(&self, _: &CallContext) -> anyhow::Result<Vec<Descriptor>> {
        Ok(vec![self.declaration()])
    }
    async fn call(
        &self,
        context: &CallContext,
        _: &str,
        call: CallToolRequestParams,
        _: &Emitter,
    ) -> Result<CallToolResult, ErrorData> {
        self.execute(
            Arc::new(context.clone()),
            Value::Object(call.arguments.unwrap_or_default()),
        )
        .await
        .map_err(crate::agent::catalog::fault)
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
