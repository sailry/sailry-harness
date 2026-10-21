use super::*;
use sailry_model_assets::verified;

/// Publishes from the validated stamp; it never inspects files again.
struct Installation {
    owner: Arc<Registry>,
    id: String,
}
impl Drop for Installation {
    fn drop(&mut self) {
        let mut state = self.owner.lock();
        if let Some(transfer) = state.transfers.remove(&self.id) {
            transfer.cancel();
        }
        self.owner.publish(&state);
    }
}

struct Removal {
    owner: Arc<Registry>,
    id: String,
    removed: bool,
}
impl Drop for Removal {
    fn drop(&mut self) {
        let mut state = self.owner.lock();
        state.removing.remove(&self.id);
        self.owner.publish(&state);
        drop(state);
        if !self.removed {
            // Files may remain after a failed removal; reinspect them off the caller.
            let owner = self.owner.clone();
            drop(self.owner.handle.spawn_blocking(move || owner.list(None)));
        }
    }
}

impl Registry {
    pub(crate) async fn install(
        self: &Arc<Self>,
        id: &str,
        cancel: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<(), Fault> {
        if cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        let definition = self.definition(id)?;
        let kind = definition.format.kind();
        super::super::availability::check(kind).await?;
        let root = self.root()?;
        let directory = files::directory(&root, id, &definition)?;
        let owner = self.clone();
        let model = id.to_owned();
        if tokio::task::spawn_blocking(move || {
            owner.verify(|candidate, _| candidate == model)?;
            Ok::<_, Fault>(owner.lock().verified.contains_key(&model))
        })
        .await
        .map_err(failure)??
        {
            return Ok(());
        }
        let (stop, mirror) = {
            let mut state = self.lock();
            let stop = self.stop.child_token();
            state.transfers.insert(id.into(), stop.clone());
            self.publish(&state);
            (stop, state.settings.mirror.clone())
        };
        let _guard = Installation {
            owner: self.clone(),
            id: id.into(),
        };
        let downloads = definition
            .assets
            .iter()
            .map(|asset| {
                Ok(sailry_model_assets::Download {
                    file: sailry_model_assets::File {
                        name: &asset.name,
                        size: asset.bytes,
                        sha256: &asset.sha256,
                    },
                    url: sailry_model_assets::url(asset, mirror.as_deref()).map_err(asset_error)?,
                })
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        let operation = async {
            sailry_model_assets::prepare(&directory, &downloads, &stop, |progress| {
                crate::installation::assets(reporter, progress);
            })
            .await
            .map_err(asset_error)?;
            if let Some(reporter) = reporter {
                reporter(sailry_protocol::installation::Phase::Validating, None);
            }
            let path = directory.clone();
            let inspecting = definition.clone();
            let stamp = tokio::task::spawn_blocking(move || {
                verified::candidate(&path, &files::assets(&inspecting))
            })
            .await
            .map_err(failure)?
            .ok_or_else(|| Fault::invalid("model assets changed before validation"))?;
            let resource = Resource {
                id: id.into(),
                definition: definition.clone(),
                directory: directory.clone(),
                stamp,
                cancel: stop.clone(),
            };
            let captured = resource.clone();
            let owner = self.clone();
            let check = Arc::new(move |_: bool| owner.installing(&captured));
            let path = directory.clone();
            let definition = definition.clone();
            let stamp = self
                .runtime
                .validate(resource.clone(), &stop, check, move |engine, cancelling| {
                    if cancelling.is_cancelled() {
                        return Err(cancelled());
                    }
                    match engine {
                        Engine::Embedding(engine) => {
                            for purpose in [
                                sailry_embedding::Input::Query,
                                sailry_embedding::Input::Document,
                            ] {
                                let vectors = engine
                                    .encode_for(purpose, &["Model compatibility check"])
                                    .map_err(super::super::runtime::embedding_error)?;
                                if vectors.len() != 1 || cancelling.is_cancelled() {
                                    return Err(cancelled());
                                }
                            }
                        }
                        Engine::Ocr(_) | Engine::Decision(_) => {}
                    }
                    files::mark(&path, &definition, &resource.stamp)
                })
                .await?;
            let mut state = self.lock();
            if stop.is_cancelled() || state.removing.contains(id) {
                return Err(cancelled());
            }
            state.verified.insert(id.into(), stamp);
            Ok(())
        };
        tokio::pin!(operation);
        tokio::select! {biased;
            _=cancel.cancelled()=>{stop.cancel();let _=operation.await;Err(cancelled())},
            result=&mut operation=>result,
        }
    }

    fn installing(&self, resource: &Resource) -> Result<(), Fault> {
        if self.stop.is_cancelled() || resource.cancel.is_cancelled() {
            return Err(cancelled());
        }
        let state = self.lock();
        if !state.transfers.contains_key(&resource.id)
            || state.removing.contains(&resource.id)
            || state.definitions.get(&resource.id) != Some(&resource.definition)
        {
            return Err(conflict());
        }
        Ok(())
    }

    pub(crate) async fn remove(self: &Arc<Self>, id: &str) -> Result<(), Fault> {
        let definition = self.definition(id)?;
        {
            let mut state = self.lock();
            state.removing.insert(id.into());
            // Clearing the stamp invalidates queued captures before files are deleted.
            state.verified.remove(id);
            self.publish(&state);
        }
        let mut guard = Removal {
            owner: self.clone(),
            id: id.into(),
            removed: false,
        };
        self.runtime.unload(id).await?;
        let root = self.root()?;
        let model = id.to_owned();
        tokio::task::spawn_blocking(move || files::remove(&root, &model, &definition))
            .await
            .map_err(failure)??;
        guard.removed = true;
        Ok(())
    }
}

fn asset_error(error: sailry_model_assets::Error) -> Fault {
    match error {
        sailry_model_assets::Error::Cancelled => cancelled(),
        sailry_model_assets::Error::InvalidAsset => {
            Fault::invalid("local model asset validation failed")
        }
        error => failure(error),
    }
}
