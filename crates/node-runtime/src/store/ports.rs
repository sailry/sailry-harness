use super::*;
use sailry_protocol::{Command, Output, Receipt, VERSION};

impl Ingress {
    pub(super) async fn port(&self, caller: NodeId, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                sailry_protocol::ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match request.command {
            Command::OpenPort { port } => {
                let stream = self.ports.prepare(caller, port)?;
                Output::PortStream { stream }
            }
            Command::CancelPort { stream } => {
                self.ports.cancel(caller, stream)?;
                Output::PortCancelled
            }
            _ => unreachable!(),
        };
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(output));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
