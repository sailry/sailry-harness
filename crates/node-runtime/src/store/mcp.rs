//! The SDK accesses OAuth grants through the existing Node database worker.
use super::database::storage_error;
use super::*;
use crate::plugins::authorization::Target;
use sailry_protocol::{Secret,EventEnvelope};

pub(super) enum Operation {
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

pub(super) fn execute(
    database: &mut Database,
    operation: Operation,
    events: &broadcast::Sender<EventEnvelope>,
) {
    match operation {
        Operation::Resolve {
            context,
            server,
            reply,
        } => {
            let _ = reply.send(plugins::connectors::mcp::resolve(
                &database.connection,
                database.profile.as_deref(),
                &context,
                &server,
            ));
        }
        Operation::Configuration { package, reply } => {
            let _ = reply.send(plugins::mcp::configuration(&database.connection, &package));
        }
        Operation::Load { target, reply } => {
            let result = plugins::connectors::mcp::load(
                &database.connection,
                database.profile.as_deref(),
                &target,
            );
            let _ = reply.send(result);
        }
        Operation::Save {
            target,
            revision,
            credentials,
            reply,
        } => {
            let result = (|| {
                let transaction = database.connection.transaction().map_err(storage_error)?;
                let summary = plugins::connectors::mcp::save(
                    &transaction,
                    database.profile.as_deref(),
                    &target,
                    revision,
                    credentials,
                )?;
                let envelopes = super::database::record(
                    &transaction,
                    database.node,
                    vec![sailry_protocol::Event::PluginChanged(summary)],
                )?;
                transaction.commit().map_err(storage_error)?;
                for envelope in envelopes {
                    let _ = events.send(envelope);
                }
                Ok(())
            })();
            let _ = reply.send(result);
        }
    }
}

impl Ingress {
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
        self.sender
            .try_send(Job::McpCredentials(Operation::Resolve {
                context,
                server,
                reply,
            }))
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
        self.sender
            .try_send(Job::McpCredentials(mcp::Operation::Load { target, reply }))
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
        self.sender
            .try_send(Job::McpCredentials(mcp::Operation::Save {
                target,
                revision,
                credentials,
                reply,
            }))
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}
