//! Public Hub discovery captures the execution Node's current mirror settings.
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
        let mirror = self.knowledge_catalog().await?.settings.mirror;
        let output = match &request.command {
            Command::SearchModels(search) => Output::ModelSearch(
                crate::models::search(
                    search,
                    mirror.as_deref(),
                    &stop,
                    #[cfg(any(test, feature = "test-support"))]
                    self.model_catalog_fixtures.get(search.hub),
                )
                .await?,
            ),
            Command::ReadModelVersions { hub, repository } => Output::ModelVersions(
                crate::models::versions(
                    *hub,
                    repository,
                    mirror.as_deref(),
                    &stop,
                    #[cfg(any(test, feature = "test-support"))]
                    self.model_catalog_fixtures.get(*hub),
                )
                .await?,
            ),
            Command::ResolveModel { kind, source } => Output::ModelDefinition(
                crate::models::resolve(
                    *kind,
                    source,
                    mirror.as_deref(),
                    &stop,
                    #[cfg(any(test, feature = "test-support"))]
                    self.model_catalog_fixtures.get(source.hub),
                )
                .await?,
            ),
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
