//! Public Hub discovery captures the execution Node's current mirror settings.
use super::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request};
use tokio::sync::oneshot;

impl Ingress {
    pub(super) async fn list_channel_presets(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let refresh = matches!(
            request.command,
            Command::ListChannelPresets {
                refresh_partners: true
            }
        );
        let (catalog, unavailable) = if refresh {
            let (catalog, unavailable) = self.presets.read(&self.closed).await?;
            ((!unavailable).then_some(catalog), unavailable)
        } else {
            (None, false)
        };
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(super::Job::ChannelPresets {
                catalog,
                unavailable,
                reply,
            })
            .map_err(|_| super::unavailable())?;
        let mut listing = response.await.map_err(|_| super::unavailable())??;
        let host = self.plugins.clone();
        let stop = self.closed.clone();
        listing = tokio::task::spawn_blocking(move || {
            for entry in &mut listing.entries {
                if let (Some(package), Some(logo)) = (&entry.package, &entry.preset.logo)
                    && logo.starts_with("dev.sailry.platform/")
                {
                    entry.icon = host
                        .channel_icon(package, logo, stop.clone())
                        .ok()
                        .flatten();
                }
            }
            listing
        })
        .await
        .map_err(|_| super::unavailable())?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::ChannelPresets(listing)));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(super) async fn model_catalog(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        // Plugin reads pass the same captured action check in ordinary Ingress admission.
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
            Command::RefreshOcrModel { id } => {
                let catalog = self.ocr_catalog().await?;
                let previous = crate::ocr::catalog::definitions(&catalog.records)?
                    .remove(id)
                    .ok_or_else(|| {
                        Fault::new(ErrorCode::NotFound, "OCR model is not registered")
                    })?;
                Output::ModelDefinition(
                    crate::models::refresh_ocr(
                        &previous,
                        mirror.as_deref(),
                        &stop,
                        #[cfg(any(test, feature = "test-support"))]
                        self.model_catalog_fixtures.get(previous.source.hub),
                    )
                    .await?,
                )
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
