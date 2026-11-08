//! Namespaced MCP tools retain the original turn, approval and package snapshot.
pub(super) mod check;
mod diagnostics;
mod input;
mod limits;
pub(super) mod management;
mod pool;
pub(super) mod warm;
pub(crate) use pool::Pool;
mod remote;
mod schema;
mod settlement;
mod status;
mod transport;

use super::catalog::Registration;
use super::*;
use crate::agent::{canonical::ToolIdentity, catalog::CallContext};
use crate::agent::{catalog::ToolProvider, engine::Emitter};
use crate::plugins::mcp::MAX_TOOLS;
use async_trait::async_trait;
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

#[derive(Default)]
pub(super) struct Connections {
    active: Vec<Arc<Connected>>,
    failure: Option<Fault>,
    images: Vec<(Vec<String>, usize)>,
}

struct Connected {
    reference: sailry_protocol::plugin::Reference,
    transport: transport::Shared,
    peer: rmcp::Peer<RoleClient>,
    toolset: Arc<remote::Remote>,
    handler: Arc<input::Handler>,
    connector: Option<crate::plugins::connectors::mcp::Authorization>,
    python: Option<crate::python::Ready>,
    reuse: bool,
    poisoned: Arc<AtomicBool>,
    closed: AtomicBool,
    retired: AtomicBool,
    used: std::sync::Mutex<std::time::Instant>,
    listing: tokio::sync::Mutex<()>,
    cleanup: Option<pool::Cleanup>,
}

impl Connected {
    async fn open(
        reference: Reference,
        launch: crate::plugins::resources::mcp::Launch,
        connector: Option<crate::plugins::connectors::mcp::Authorization>,
        reuse: bool,
        stop: &CancellationToken,
        environment: &crate::process::Environment,
    ) -> Result<Self, Fault> {
        let python = match &launch {
            crate::plugins::resources::mcp::Launch::Stdio { python, .. } => python.clone(),
            _ => None,
        };
        let handler = Arc::new(input::Handler::new());
        let mut transport_connector = connector.clone();
        if reuse && let Some(connector) = &mut transport_connector {
            connector.settings = None;
        }
        let (mut transport, service) = transport::Connection::open(
            launch,
            transport_connector,
            stop,
            handler.clone(),
            !reuse,
            environment,
        )
        .await?;
        service
            .peer()
            .set_response_cache_config(
                rmcp::ClientCacheConfig::default()
                    .with_default_ttl(rmcp::MAX_CLIENT_CACHE_TTL)
                    .with_serve_stale_on_error(false),
            )
            .await;
        let peer = service.peer().clone();
        let toolset = remote::Remote::new(service);
        transport.track_tasks(toolset.clone());
        Ok(Self {
            reference,
            transport: Arc::new(tokio::sync::Mutex::new(transport)),
            peer,
            toolset,
            handler,
            connector,
            python,
            reuse,
            poisoned: Arc::default(),
            closed: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            used: std::sync::Mutex::new(std::time::Instant::now()),
            listing: tokio::sync::Mutex::new(()),
            cleanup: None,
        })
    }

    fn healthy(&self) -> bool {
        !self.poisoned.load(Ordering::Relaxed) && !self.toolset.is_closed()
    }

    async fn close(&self) -> Result<(), Fault> {
        self.poisoned.store(true, Ordering::Relaxed);
        let result = self.transport.lock().await.close().await;
        self.closed.store(true, Ordering::Relaxed);
        result
    }

    async fn release(self: &Arc<Self>) -> Result<(), Fault> {
        *self.used.lock().unwrap() = std::time::Instant::now();
        if self.reuse
            && (Arc::strong_count(self) > 1
                || (self.healthy() && !self.retired.load(Ordering::Relaxed)))
        {
            Ok(())
        } else {
            self.close().await
        }
    }

    fn retain(&self, task: tokio::task::JoinHandle<()>) {
        if let Some(cleanup) = &self.cleanup {
            let mut cleanup = cleanup.lock().unwrap();
            cleanup.retain(|task| !task.is_finished());
            cleanup.push(task);
        }
    }
}

impl Drop for Connected {
    fn drop(&mut self) {
        if self.reuse && !self.closed.load(Ordering::Relaxed) {
            let transport = self.transport.clone();
            let package = self.reference.clone();
            let task = tokio::spawn(async move {
                if let Err(error) = transport.lock().await.close().await {
                    let mut entry = crate::logging::Entry::new(
                        crate::logging::Area::Run,
                        crate::logging::Phase::Failed,
                    );
                    entry.context.package = Some(package);
                    entry.stage = Some("mcp_server");
                    entry.code = Some(error.code);
                    entry.reason = Some("connection_cleanup");
                    entry.emit_warn();
                }
            });
            self.retain(task);
        }
    }
}

impl Connections {
    pub(super) async fn bind(
        &mut self,
        resources: &crate::plugins::resources::Resources,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        stop: &CancellationToken,
        assistant: Option<(&Reference, &Declaration)>,
    ) -> (Vec<Registration>, String) {
        let startup = super::diagnostics::Startup::new(ingress.node, invocation, stop);
        let started = std::time::Instant::now();
        self.images.clear();
        let mut tools: Vec<Registration> = Vec::new();
        let mut diagnostics = String::new();
        if let Err(error) = self.release().await {
            self.failure = Some(error);
        }
        let configurations = resources.mcp(&ingress.environment).await;
        let mut selected: Vec<_> = configurations.into_iter().filter(|config| {
            assistant.is_none_or(|(package, declaration)| {
                config.reference == *package
                    && declaration.tools.iter().any(|tool| {
                        matches!(tool, SelectedTool::Plugin { server: Some(server), .. } if server == &config.name)
                    })
            })
        }).collect();
        if stop.is_cancelled() {
            selected.clear();
        }
        let claims: Vec<_> = selected
            .iter_mut()
            .map(|config| std::mem::replace(&mut config.launch, Err(unavailable())))
            .collect();
        let attempts = futures::future::join_all(selected.iter().zip(claims).enumerate().map(
            |(index, (config, launch))| {
                let target = Target {
                    resources,
                    ingress,
                    invocation: Some(invocation),
                    stop,
                };
                target.connect(index, config, launch)
            },
        ))
        .await;
        for (config, attempt) in selected.iter().zip(attempts) {
            let over_limit = attempt
                .result
                .as_ref()
                .is_ok_and(|(_, descriptors, _)| tools.len() + descriptors.len() > MAX_TOOLS);
            let result = match attempt.result {
                Ok((connection, descriptors, connector))
                    if tools.len() + descriptors.len() <= MAX_TOOLS =>
                {
                    let exclusive = config
                        .options
                        .exclusive
                        .then(|| ingress.agents.mcp.service(&config.plugin, &config.name));
                    let closed = connection.transport.lock().await.stopped();
                    if let Some(keep) = config.options.keep_images {
                        self.images.push((
                            descriptors
                                .iter()
                                .map(|descriptor| {
                                    alias(&config.plugin, &config.name, &descriptor.name)
                                })
                                .collect(),
                            keep,
                        ));
                    } else {
                        self.images
                            .extend(descriptors.iter().filter_map(|descriptor| {
                                let keep = resources
                                    .tool_declaration(
                                        &config.plugin,
                                        &config.name,
                                        &descriptor.name,
                                    )?
                                    .keep_images?;
                                Some((
                                    vec![alias(&config.plugin, &config.name, &descriptor.name)],
                                    keep,
                                ))
                            }));
                    }
                    for descriptor in descriptors {
                        let presentation = resources.tool_presentation(
                            &config.plugin,
                            &config.name,
                            &descriptor.name,
                        );
                        let grouping =
                            resources.tool_grouping(&config.plugin, &config.name, &descriptor.name);
                        let display = resources
                            .tool_display(&config.plugin, &config.name, &descriptor.name)
                            .or_else(|| {
                                resources
                                    .tool_declaration(
                                        &config.plugin,
                                        &config.name,
                                        &descriptor.name,
                                    )
                                    .is_none()
                                    .then(|| {
                                        let mut navigation =
                                            sailry_protocol::plugin::desktop::Navigation {
                                                label: descriptor.name.to_string(),
                                                icon: None,
                                                locales: Default::default(),
                                            };
                                        if !navigation.valid() {
                                            navigation.label = alias(
                                                &config.plugin,
                                                &config.name,
                                                &descriptor.name,
                                            );
                                        }
                                        let mut display =
                                            sailry_protocol::tool::Display::from(navigation);
                                        display.raw_result = true;
                                        display
                                    })
                            });
                        let bound = Arc::new(Bound {
                            connection: connection.clone(),
                            name: alias(&config.plugin, &config.name, &descriptor.name),
                            description: format!(
                                "MCP {}/{}/{}\n{}",
                                config.plugin,
                                config.name,
                                descriptor.name,
                                descriptor.description.as_deref().unwrap_or("")
                            ),
                            descriptor,
                            package: connection.reference.clone(),
                            toolset: connection.toolset.clone(),
                            ingress: ingress.clone(),
                            handler: connection.handler.clone(),
                            poisoned: connection.poisoned.clone(),
                            reuse: connection.reuse,
                            turn: invocation.turn.id,
                            session: invocation.turn.session.to_string(),
                            stop: stop.clone(),
                            connector: connector.clone(),
                            python: connection.python.clone(),
                            exclusive: exclusive.clone(),
                            transport: connection.transport.clone(),
                            closed: closed.clone(),
                        });
                        let mut registration =
                            Registration::new(bound.declaration(), bound.clone(), presentation);
                        registration.grouping = grouping;
                        registration.display =
                            display.map(|display| (registration.tool.name.to_string(), display));
                        registration.plugin = Some(config.plugin.clone());
                        tools.push(registration);
                    }
                    self.active.push(connection);
                    Ok(())
                }
                Ok((connection, _, _)) => connection.release().await.and(Err(unavailable())),
                Err(error) => Err(error),
            };
            let mut entry = startup.entry(
                "mcp_server",
                started,
                result.as_ref().err().map(|error| error.code),
            );
            entry.context.package = Some(config.reference.clone());
            entry.context.handler = Some(config.name.clone());
            entry.connection_reused = Some(attempt.reused);
            if let Err(error) = &result {
                entry.reason = Some(if error.code == ErrorCode::Cancelled {
                    "cancelled"
                } else if over_limit {
                    "tool_limit"
                } else {
                    attempt.step
                });
            }
            entry.duration_us =
                Some(u64::try_from(attempt.elapsed.as_micros()).unwrap_or(u64::MAX));
            startup.record(entry);
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
        let mut entry = startup.entry("mcp_bind", started, None);
        entry.count = Some(selected.len() as u64);
        startup.record(entry);
        (tools, diagnostics)
    }

    pub(super) fn screenshots(&self) -> Vec<(Vec<String>, usize)> {
        self.images.clone()
    }

    async fn release(&mut self) -> Result<(), Fault> {
        let active = std::mem::take(&mut self.active);
        futures::future::join_all(
            active
                .into_iter()
                .map(|connection| async move { connection.release().await }),
        )
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

struct Attempt {
    reused: bool,
    /// The connection step a failure stopped at, for diagnostics.
    step: &'static str,
    elapsed: Duration,
    result: Result<
        (
            Arc<Connected>,
            Vec<Descriptor>,
            Option<crate::plugins::connectors::mcp::Authorization>,
        ),
        Fault,
    >,
}

/// Turn inputs shared by the concurrent server attempts of one bind.
#[derive(Clone, Copy)]
struct Target<'a> {
    resources: &'a crate::plugins::resources::Resources,
    ingress: &'a Arc<Ingress>,
    invocation: Option<&'a Invocation>,
    stop: &'a CancellationToken,
}

impl Target<'_> {
    /// Authorize, reuse or open, then list one server within its own step bounds.
    async fn connect(
        self,
        index: usize,
        config: &crate::plugins::resources::mcp::Configured,
        launch: Result<crate::plugins::resources::mcp::Launch, Fault>,
    ) -> Attempt {
        let Self {
            resources,
            ingress,
            invocation,
            stop,
        } = self;
        let started = std::time::Instant::now();
        let mut attempt = ingress.agents.mcp.begin(&config.reference, &config.name);
        let mut reused = false;
        let mut step = "launch";
        let result = async {
            if index >= crate::plugins::mcp::MAX_SERVERS { return Err(unavailable()); }
            let mut launch = launch?;
            let context=sailry_protocol::plugin::Context{package:config.reference.clone(),worktree:invocation.map(|invocation| invocation.worktree),session:invocation.map(|invocation|invocation.turn.session),turn:invocation.map(|invocation|invocation.turn.id),invocation:None,surface:Default::default()};
            let reuse = config.options.reuse && ingress.agents.mcp.reusable(&config.reference);
            if invocation.is_none() && !reuse {
                return Err(unavailable());
            }
            step = "authorization";
            let connector = tokio::select! {
                _ = stop.cancelled() => return Err(cancelled()),
                result = tokio::time::timeout(Duration::from_secs(10), resources.mcp_authorization(ingress, context, &config.name, &mut launch)) => result.map_err(|_| unavailable())??,
            };
            step = "connection";
            let connection = if reuse {
                let mut transport_connector = connector.clone();
                if let Some(connector) = &mut transport_connector {
                    connector.stop = ingress.agents.stopped.clone();
                }
                let (connection, pooled) = ingress.agents.mcp.lease(config, launch, transport_connector, ingress, pool::Lease { stop, retry_failed: invocation.is_some(), attempt: &mut attempt }).await?;
                reused = pooled;
                connection
            } else {
                ingress.agents.mcp.started(&config.reference, &config.name);
                Arc::new(Connected::open(config.reference.clone(), launch, connector.clone(), false, stop, &ingress.environment).await?)
            };
            let mut caller = connection.connector.clone();
            if let (Some(caller), Some(connector)) = (&mut caller, connector) {
                caller.settings = connector.settings;
                caller.stop = stop.clone();
            }
            step = "tool_list";
            let listing = async {
                let _listing = connection.listing.lock().await;
                if !connection.healthy() {
                    return Err(unavailable());
                }
                list(&connection.peer, config.tools.as_deref()).await
            };
            let listed = tokio::select! {
                _ = stop.cancelled() => {
                    connection.release().await?;
                    return Err(cancelled());
                }
                result = tokio::time::timeout(Duration::from_secs(10), listing) => result.unwrap_or_else(|_| Err(unavailable())),
            };
            match listed {
                Ok(descriptors) if connection.healthy() => Ok((connection, descriptors, caller)),
                _ => {
                    if !connection.reuse {
                        connection.close().await?;
                    }
                    Err(unavailable())
                }
            }
        }.await;
        let status = if stop.is_cancelled() {
            Err(cancelled())
        } else {
            result
                .as_ref()
                .map(|(_, tools, _)| tools.len())
                .map_err(Clone::clone)
        };
        ingress.agents.mcp.finish(attempt, status);
        Attempt {
            reused,
            step,
            elapsed: started.elapsed(),
            result,
        }
    }
}

async fn list(
    service: &rmcp::Peer<RoleClient>,
    selected: Option<&[String]>,
) -> Result<Vec<Descriptor>, Fault> {
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
            // Undeclared tools are neither registered nor counted against the limits.
            if selected
                .is_some_and(|selected| !selected.iter().any(|name| name.as_str() == &*tool.name))
            {
                continue;
            }
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

pub(super) fn is_alias(name: &str) -> bool {
    name.strip_prefix("mcp_")
        .and_then(|suffix| suffix.rsplit_once('_'))
        .is_some_and(|(label, digest)| {
            !label.is_empty()
                && label.len() <= 32
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                && digest.len() == 24
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

struct Bound {
    connection: Arc<Connected>,
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
    reuse: bool,
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
        if self.reuse {
            self.ingress.agents.mcp.failed(&self.connection);
        }
        let failure = match &result {
            Ok(value) if value.is_error == Some(true) => {
                Some(sailry_protocol::tool::Failure::tool(None))
            }
            Err(error) => Some(sailry_protocol::tool::Failure::runtime(Some(error.code))),
            _ => None,
        };
        if let Some(failure) = failure
            && let Some(timeline) = self.ingress.agents.timeline(self.turn)
            && timeline
                .tool_failure(context.branch(), context.function_call_id(), self.name())
                .is_none()
        {
            timeline.fail_tool(
                context.branch(),
                context.function_call_id(),
                self.name(),
                failure,
                None,
            );
        }
        diagnostics::finish(
            &mut span,
            &result,
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
            || self.poisoned.load(Ordering::Relaxed)
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
                guard = self.handler.serial.clone().lock_owned() => guard,
            };
            Ok::<_, Fault>((service, serial))
        };
        let (service, serial) = diagnostics::stage(
            span,
            &self.stop,
            "lock_wait",
            ErrorCode::Unavailable,
            input::wait(&self.stop, &self.closed, waiting),
        )
        .await?;
        if context.is_cancelled()
            || self.stop.is_cancelled()
            || self.closed.is_cancelled()
            || self.poisoned.load(Ordering::Relaxed)
        {
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
        let failure = active.failure();
        let (service, mut call) = if self.reuse {
            (service, None)
        } else {
            (
                None,
                service.map(|guard| {
                    transport::Call::new(
                        self.transport.clone(),
                        self.closed.clone(),
                        guard,
                        self.poisoned.clone(),
                    )
                }),
            )
        };
        let pending = Pending((!self.reuse).then_some(&*self.poisoned));
        let operation = async {
            if self.reuse {
                settlement::Owner {
                    connection: self.connection.clone(),
                    active,
                    service,
                    serial,
                    shutdown: self.ingress.agents.stopped.clone(),
                    ingress: self.ingress.clone(),
                }
                .run(self.descriptor.name.to_string(), args.clone())
                .await
            } else {
                let _serial = serial;
                let stopped = active.stopped();
                active
                    .run(
                        self.toolset
                            .call(&self.descriptor.name, args.clone(), &stopped),
                    )
                    .await
            }
        };
        let result = tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::invalid("MCP invocation was cancelled; remote effects may be uncertain")),
            result = operation => {
                pending.answered();
                if matches!(&result, Ok(Ok(_))) {
                    if let Some(call) = &mut call { call.confirmed(); }
                } else if !self.reuse { self.poisoned.store(true, Ordering::Relaxed); }
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
                        if let Some(observed) = failure.lock().unwrap().take()
                            && let Some(timeline) = self.ingress.agents.timeline(self.turn) {
                            let mut message = observed.message.map(Value::String).unwrap_or(Value::Null);
                            if let Some(connector) = &self.connector { connector.redact(&mut message); }
                            timeline.fail_tool(context.branch(), context.function_call_id(), self.name(), observed.failure, message.as_str().map(str::to_owned));
                        }
                        Err(Fault::new(ErrorCode::OutcomeUnknown, format!("MCP call failed without replay; remote effects may be uncertain: {}", error.message)))
                    },
                    Err(_) => Err(Fault::new(ErrorCode::OutcomeUnknown, "MCP result is unavailable; remote effects may be uncertain")),
                }
            }
        };
        diagnostics::finish(&mut external, &result, self.stop.is_cancelled());
        result
    }
}

/// Nonreusable calls retain their transport until remote cleanup is confirmed.
struct Pending<'a>(Option<&'a AtomicBool>);

impl Pending<'_> {
    fn answered(mut self) {
        self.0 = None;
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if let Some(poisoned) = self.0 {
            poisoned.store(true, Ordering::Relaxed);
        }
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

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "MCP connection was cancelled")
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
