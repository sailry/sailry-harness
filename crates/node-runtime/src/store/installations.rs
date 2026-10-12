//! Transient task observation and the existing bounded installation queue.
mod registry;
mod worker;

pub(super) use registry::Registry;
pub(super) use worker::Worker;

use super::{Ingress, unavailable};
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request};
use tokio::sync::oneshot;

impl Ingress {
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
