//! The SDK accesses OAuth grants through the existing Node database worker.
use super::database::storage_error;
use super::*;
use crate::plugins::authorization::Target;
use sailry_protocol::{EventEnvelope, Secret};

pub(super) enum Operation {
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

impl Ingress {
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
