//! Public hub discovery uses the same authenticated Node boundary without Store I/O.
use super::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request};
use tokio::sync::oneshot;

impl Ingress {
    pub(super) async fn model_catalog(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if request.plugin.is_some() {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "model discovery requires a controller",
            ));
        }
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let output = match &request.command {
            Command::SearchModels(search) => {
                Output::ModelSearch(crate::models::search(search, &stop).await?)
            }
            Command::ReadModelVersions { hub, repository } => {
                Output::ModelVersions(crate::models::versions(*hub, repository, &stop).await?)
            }
            Command::ResolveModel { kind, source } => {
                Output::ModelDefinition(crate::models::resolve(*kind, source, &stop).await?)
            }
            _ => return Err(Fault::invalid("model discovery expected")),
        };
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
