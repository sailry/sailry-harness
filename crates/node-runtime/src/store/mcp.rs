//! The SDK accesses OAuth grants through the existing Node database worker.
use super::database::storage_error;
use super::*;
use crate::plugins::authorization::Target;
use sailry_protocol::Secret;

pub(super) enum Operation {
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

pub(super) fn execute(database: &mut Database, operation: Operation) {
    match operation {
        Operation::Configuration { package, reply } => {
            let _ = reply.send(plugins::mcp::configuration(&database.connection, &package));
        }
        Operation::Load { target, reply } => {
            let result = plugins::grants::load(
                &database.connection,
                &target.plugin,
                &target.server,
                &target.endpoint,
                target.id,
            );
            let result = match result {
                Ok(value) => Ok(Some(value)),
                Err(error) if error.code == ErrorCode::NotConfigured => Ok(None),
                Err(error) => Err(error),
            };
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
                let (current, _) = plugins::grants::load(
                    &transaction,
                    &target.plugin,
                    &target.server,
                    &target.endpoint,
                    target.id,
                )?;
                if current != revision {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "MCP credential changed",
                    ));
                }
                plugins::grants::renew(
                    &transaction,
                    &target.plugin,
                    &target.server,
                    &target.endpoint,
                    target.id,
                    revision,
                    credentials,
                )?;
                transaction.commit().map_err(storage_error)
            })();
            let _ = reply.send(result);
        }
    }
}

impl Ingress {
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
