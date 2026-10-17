//! The SDK accesses OAuth grants through the existing Node database worker.
use super::database::storage_error;
use super::*;
use crate::plugins::authorization::Target;
use sailry_protocol::Secret;

pub(super) enum Operation {
    Inspect {
        package: sailry_protocol::plugin::Reference,
        server: String,
        reply: oneshot::Sender<Result<Inspection, Fault>>,
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

pub(super) struct Inspection {
    pub info: sailry_protocol::plugin::Info,
    pub status: sailry_protocol::plugin::authorization::Status,
    pub settings: crate::plugins::settings::Resolved,
    pub registered: bool,
    pub previous_endpoint: Option<String>,
}

pub(super) fn execute(database: &mut Database, operation: Operation) {
    match operation {
        Operation::Inspect {
            package,
            server,
            reply,
        } => {
            let _ = reply.send(plugins::authorization::inspect(
                &database.connection,
                &package,
                &server,
            ));
        }
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
    pub(super) async fn read_mcp_authorization(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        use sailry_protocol::{Command, Output, Receipt};
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let Command::ReadMcpAuthorization { package, server } = &request.command else {
            return Err(Fault::invalid("MCP authorization inspection expected"));
        };
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::McpCredentials(Operation::Inspect {
                package: package.clone(),
                server: server.clone(),
                reply,
            }))
            .map_err(|_| unavailable())?;
        let mut inspection = response.await.map_err(|_| unavailable())??;
        let name = package.name.clone();
        let resources = crate::plugins::resources::Resources::new(
            self.plugins.clone(),
            vec![inspection.info],
            [(name.clone(), Ok(inspection.settings))].into(),
            self.closed.clone(),
        );
        if let Ok(endpoint) = resources.mcp_endpoint(&name, server).await {
            inspection.status.can_authorize = tokio::select! {
                _ = self.closed.cancelled() => return Err(unavailable()),
                available = self.plugins.authorizations.available(package, server, &endpoint,
                    inspection.registered || inspection.previous_endpoint.as_deref() == Some(endpoint.as_str())) => available,
            };
        }
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::McpAuthorization(inspection.status)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
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
