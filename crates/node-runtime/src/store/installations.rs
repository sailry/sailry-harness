//! Transient task observation and the existing bounded installation queue.
pub(super) mod models;
mod registry;
mod worker;

pub(super) use registry::Registry;
pub(super) use worker::Worker;

use super::{Ingress, unavailable};
use sailry_link::{Admission, Response};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request};
use tokio::sync::oneshot;

pub(super) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
    /// Resource calls hold their admitted Agent operation until durable completion.
    pub operation: bool,
}

impl Ingress {
    pub(super) fn read_installations(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let Command::ReadInstallations { requests } = request.command else {
            return Err(Fault::invalid("installation observation expected"));
        };
        let tasks = self
            .installations
            .read(caller, &requests, request.plugin.as_ref())?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::Installations(tasks)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(super) fn dismiss_installation(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let Command::DismissInstallation { request: id } = request.command else {
            return Err(Fault::invalid("installation dismissal expected"));
        };
        self.installations
            .dismiss(caller, id, request.plugin.as_ref())?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::InstallationDismissed { request: id }));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
