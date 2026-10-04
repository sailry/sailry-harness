use crate::store::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Receipt, Request, VERSION};
use tokio::sync::oneshot;

impl Ingress {
    pub(in crate::store) async fn plugin_catalog(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let result = match &request.command {
            Command::SearchPluginCatalog {
                source,
                query,
                page,
            } => Output::PluginCatalog(
                self.plugins
                    .catalog(*source, query, *page, self.closed.clone())
                    .await?,
            ),
            Command::ReadCatalogPlugin { id } => Output::PluginRepository(
                self.plugins.catalog_source(id, self.closed.clone()).await?,
            ),
            Command::ReadCatalogPluginInfo { source, id } => {
                let stop = self.closed.child_token();
                let _guard = stop.clone().drop_guard();
                Output::Plugin(self.plugins.catalog_info(*source, id, stop).await?)
            }
            Command::InspectPluginSource { source } => Output::PluginSource(
                self.plugins
                    .inspect_source(source, self.closed.clone())
                    .await?,
            ),
            _ => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "plugin catalog command expected",
                ));
            }
        };
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(result));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
