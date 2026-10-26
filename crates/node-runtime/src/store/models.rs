//! Public Hub discovery captures the execution Node's current mirror settings.
use super::Ingress;
use sailry_link::Admission;
use sailry_protocol::{Command, Fault, NodeId, Output, Request};
use tokio::sync::oneshot;

impl Ingress {
    pub(super) async fn list_channel_presets(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        let refresh = matches!(
            request.command,
            Command::ListChannelPresets {
                refresh_partners: true
            }
        );
        let mut listing = if refresh {
            if request.plugin.is_some() {
                self.check_plugin(caller, request.clone()).await?;
            }
            let (catalog, unavailable) = self.presets.read(&self.closed).await?;
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(super::Job::ChannelPresets {
                    catalog: (!unavailable).then_some(catalog),
                    unavailable,
                    reply,
                })
                .map_err(|_| super::unavailable())?;
            response.await.map_err(|_| super::unavailable())??
        } else {
            let storage = self.storage.get().ok_or_else(super::unavailable)?;
            let database = storage
                .context(true)
                .await
                .map_err(super::database::storage_error)?;
            let transaction = database
                .connection
                .unchecked_transaction()
                .await
                .map_err(super::database::storage_error)?;
            let result = async {
                database.check_plugin(caller, &request).await?;
                super::providers::presets::list(&transaction, None, false).await
            }
            .await;
            match result {
                Ok(listing) => {
                    transaction
                        .commit()
                        .await
                        .map_err(super::database::storage_error)?;
                    listing
                }
                Err(error) => {
                    transaction
                        .rollback()
                        .await
                        .map_err(super::database::storage_error)?;
                    return Err(error);
                }
            }
        };
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
        // Plugin reads pass the same captured action check in ordinary Ingress admission.
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let mirror = self.local_models.settings().mirror;
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
            Command::RefreshLocalModel { id } => {
                let previous = self.local_models.definition(id)?;
                if previous.format.kind() != sailry_protocol::models::Kind::Ocr {
                    return Err(Fault::invalid(
                        "only OCR model definitions can be refreshed",
                    ));
                }
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
