//! The SDK accesses OAuth grants through the existing Node database worker.
use super::database::storage_error;
use super::*;
use crate::plugins::authorization::Target;
use sailry_protocol::{EventEnvelope, Secret};
use std::collections::BTreeMap;
use turso::Connection;

type Resources = Vec<(
    sailry_protocol::plugin::Info,
    Result<crate::plugins::settings::Resolved, Fault>,
)>;
type Authorizations = BTreeMap<String, Vec<(sailry_protocol::plugin::connector::Id, u64)>>;

pub(super) enum Operation {
    Resources {
        package: Option<String>,
        reply: oneshot::Sender<Result<Resources, Fault>>,
    },
    Authorizations {
        package: Option<String>,
        reply: oneshot::Sender<Result<Authorizations, Fault>>,
    },
    Preparation {
        package: sailry_protocol::plugin::Reference,
        server: String,
        reply: oneshot::Sender<
            Result<
                (
                    sailry_protocol::plugin::Info,
                    crate::plugins::settings::Resolved,
                ),
                Fault,
            >,
        >,
    },
    ComputerPermissions {
        caller: sailry_protocol::NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<
            Result<
                (
                    sailry_protocol::plugin::Info,
                    crate::plugins::settings::Resolved,
                ),
                Fault,
            >,
        >,
    },
    Resolve {
        context: sailry_protocol::plugin::Context,
        server: String,
        reply: oneshot::Sender<
            Result<
                Vec<(
                    crate::plugins::connectors::Target,
                    sailry_protocol::plugin::connector::Injection,
                )>,
                Fault,
            >,
        >,
    },
    Configuration {
        package: sailry_protocol::plugin::Reference,
        reply: oneshot::Sender<Result<Option<sailry_protocol::plugin::mcp::Configuration>, Fault>>,
    },
    Load {
        target: Target,
        reply: oneshot::Sender<Result<Option<(u64, Secret)>, Fault>>,
    },
    Save {
        target: Target,
        revision: u64,
        credentials: Option<Secret>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
}

pub(super) async fn execute(
    database: &mut Database,
    operation: Operation,
    events: &broadcast::Sender<EventEnvelope>,
) {
    match operation {
        Operation::Resources { package, reply } => {
            let result = async {
                let mut resources = Vec::new();
                for summary in plugins::list(&database.connection).await? {
                    if package.as_ref().is_some_and(|name| *name != summary.name) {
                        continue;
                    }
                    if let Some(info) = plugins::get(&database.connection, &summary.name).await?
                        && !info.mcp.is_empty()
                    {
                        let settings =
                            plugins::settings::resolve(&database.connection, &info).await;
                        resources.push((info, settings));
                    }
                }
                Ok(resources)
            }
            .await;
            let _ = reply.send(result);
        }
        Operation::Authorizations { package, reply } => {
            let result = authorizations(
                &database.connection,
                database.profile.as_deref(),
                package.as_deref(),
            )
            .await;
            let _ = reply.send(result);
        }
        Operation::Preparation {
            package,
            server,
            reply,
        } => {
            let _ =
                reply.send(plugins::mcp::prepare(&database.connection, &package, &server).await);
        }
        Operation::ComputerPermissions {
            caller,
            request,
            reply,
        } => {
            let result = (async || {
                database.check_plugin(caller, &request).await?;
                let info = database.plugin_package(caller, &request).await?;
                if request
                    .plugin
                    .as_ref()
                    .is_none_or(|context| context.package != info.summary.reference())
                {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "computer permission package changed",
                    ));
                }
                let settings = plugins::settings::resolve(&database.connection, &info).await?;
                Ok((info, settings))
            })()
            .await;
            let _ = reply.send(result);
        }
        Operation::Resolve {
            context,
            server,
            reply,
        } => {
            let _ = reply.send(
                plugins::connectors::mcp::resolve(
                    &database.connection,
                    database.profile.as_deref(),
                    &context,
                    &server,
                )
                .await,
            );
        }
        Operation::Configuration { package, reply } => {
            let _ = reply.send(plugins::mcp::configuration(&database.connection, &package).await);
        }
        Operation::Load { target, reply } => {
            let result = plugins::connectors::mcp::load(
                &database.connection,
                database.profile.as_deref(),
                &target,
            )
            .await;
            let _ = reply.send(result);
        }
        Operation::Save {
            target,
            revision,
            credentials,
            reply,
        } => {
            let result = (async || {
                let publication = database.publication.clone();
                let transaction = database
                    .connection
                    .unchecked_transaction()
                    .await
                    .map_err(storage_error)?;
                let summary = plugins::connectors::mcp::save(
                    &transaction,
                    database.profile.as_deref(),
                    &target,
                    revision,
                    credentials,
                )
                .await?;
                let envelopes = super::database::record(
                    &transaction,
                    database.node,
                    vec![sailry_protocol::Event::PluginChanged(summary)],
                )
                .await?;
                let publication = publication.lock().await;
                crate::store::database::conflict::commit(transaction).await?;
                for envelope in envelopes {
                    publication.send(events, envelope);
                }
                Ok(())
            })()
            .await;
            let _ = reply.send(result);
        }
    }
}

async fn authorizations(
    db: &Connection,
    profile: Option<&std::path::Path>,
    package: Option<&str>,
) -> Result<Authorizations, Fault> {
    let mut authorized = Authorizations::new();
    for record in plugins::connectors::records(db).await? {
        if package.is_some_and(|package| package != record.plugin) {
            continue;
        }
        let target = match plugins::connectors::current_target(
            db,
            profile,
            record.id,
            record.auth_revision,
            false,
        )
        .await
        {
            Ok(target) => target,
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::NotConfigured
                        | ErrorCode::NotFound
                        | ErrorCode::RevisionConflict
                        | ErrorCode::PermissionDenied
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if target
            .declaration
            .inject
            .iter()
            .any(|injection| injection.server.is_some())
        {
            authorized
                .entry(record.plugin)
                .or_default()
                .push((record.id, record.auth_revision));
        }
    }
    Ok(authorized)
}

impl Ingress {
    async fn mcp_authorizations(&self, package: Option<String>) -> Result<Authorizations, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(Operation::Authorizations {
            package,
            reply,
        }))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn mcp_resources(
        &self,
        package: Option<String>,
    ) -> Result<crate::plugins::resources::Resources, Fault> {
        let complete = package.is_none();
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(Operation::Resources { package, reply }))
            .await
            .map_err(|_| unavailable())?;
        let resources = response.await.map_err(|_| unavailable())??;
        let mut packages = Vec::new();
        let mut settings = std::collections::BTreeMap::new();
        for (info, resolved) in resources {
            self.agents.mcp.observe(&info.summary);
            if !info.summary.available() {
                continue;
            }
            settings.insert(info.summary.name.clone(), resolved);
            packages.push(info);
        }
        if complete {
            self.agents.mcp.finish_inventory();
        }
        Ok(crate::plugins::resources::Resources::new(
            self.plugins.clone(),
            packages,
            settings,
            self.agents.stopped.clone(),
        ))
    }

    pub(crate) async fn mcp_preparation(
        &self,
        package: sailry_protocol::plugin::Reference,
        server: String,
    ) -> Result<
        (
            sailry_protocol::plugin::Info,
            crate::plugins::settings::Resolved,
        ),
        Fault,
    > {
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(Operation::Preparation {
            package,
            server,
            reply,
        }))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
    pub(crate) async fn computer_inputs(
        &self,
        caller: sailry_protocol::NodeId,
        request: Request,
    ) -> Result<
        (
            sailry_protocol::plugin::Info,
            crate::plugins::settings::Resolved,
        ),
        Fault,
    > {
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(Operation::ComputerPermissions {
            caller,
            request: Box::new(request),
            reply,
        }))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn mcp_connectors(
        &self,
        context: sailry_protocol::plugin::Context,
        server: String,
    ) -> Result<
        Vec<(
            crate::plugins::connectors::Target,
            sailry_protocol::plugin::connector::Injection,
        )>,
        Fault,
    > {
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(Operation::Resolve {
            context,
            server,
            reply,
        }))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn mcp_credentials(
        &self,
        target: Target,
    ) -> Result<Option<(u64, Secret)>, Fault> {
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(mcp::Operation::Load { target, reply }))
            .await
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn save_mcp_credentials(
        &self,
        target: Target,
        revision: u64,
        credentials: Option<Secret>,
    ) -> Result<(), Fault> {
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.submit(Job::McpCredentials(mcp::Operation::Save {
            target,
            revision,
            credentials,
            reply,
        }))
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}

pub(super) fn start(ingress: &Arc<Ingress>) {
    let ingress = ingress.clone();
    let mut events = ingress.events.subscribe();
    tokio::spawn(async move {
        let stop = ingress.agents.stopped.clone();
        let mut authorized = ingress.mcp_authorizations(None).await.unwrap_or_default();
        let mut expiry = tokio::time::interval(std::time::Duration::from_secs(60));
        expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut warming: Pending<'_, ()> = Box::pin(crate::agent::warm_mcp(&ingress, None, &stop));
        let mut active = true;
        let mut pending = std::collections::BTreeSet::new();
        loop {
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                _ = ingress.closed.cancelled() => break,
                event = events.recv() => {
                    let package = match event {
                        Ok(event) => match event.event {
                            sailry_protocol::Event::PluginChanged(summary) => {
                                let changed = match ingress.agents.mcp.refresh(&summary).await {
                                    Ok(changed) => changed,
                                    Err(error) => {
                                        cleanup(&ingress, Err(error));
                                        false
                                    }
                                };
                                let connected = match ingress
                                    .mcp_authorizations(Some(summary.name.clone())).await
                                {
                                    Ok(mut current) => {
                                        let current = current.remove(&summary.name).unwrap_or_default();
                                        let previous = authorized.insert(summary.name.clone(), current.clone()).unwrap_or_default();
                                        current.iter().any(|target| !previous.contains(target))
                                    }
                                    Err(error) => {
                                        cleanup(&ingress, Err(error));
                                        false
                                    }
                                };
                                if !changed && !connected {
                                    continue;
                                }
                                Some(summary.name)
                            }
                            sailry_protocol::Event::PluginRemoved { name } => {
                                authorized.remove(&name);
                                cleanup(&ingress, ingress.agents.mcp.invalidate(Some(&name)).await);
                                Some(name)
                            }
                            _ => continue,
                        },
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if let Ok(current) = ingress.mcp_authorizations(None).await {
                                authorized = current;
                            }
                            None
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    if active && let Some(package) = package {
                        pending.insert(package);
                    } else {
                        warming = Box::pin(crate::agent::warm_mcp(&ingress, package, &stop));
                        active = true;
                    }
                }
                _ = &mut warming, if active => {
                    if let Some(package) = pending.pop_first() {
                        warming = Box::pin(crate::agent::warm_mcp(&ingress, Some(package), &stop));
                    } else {
                        active = false;
                    }
                },
                _ = expiry.tick() => {
                    cleanup(&ingress, ingress.agents.mcp.expire().await);
                }
            }
        }
    });
}

fn cleanup(ingress: &Ingress, result: Result<(), Fault>) {
    if let Err(error) = result {
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Failed);
        entry.context.node = Some(ingress.node);
        entry.stage = Some("mcp_server");
        entry.code = Some(error.code);
        entry.reason = Some("connection_cleanup");
        entry.emit_warn();
    }
}
