use super::*;
use sailry_model_assets::verified;

struct Installation {
    owner: Arc<Registry>,
    id: String,
}
impl Drop for Installation {
    fn drop(&mut self) {
        let mut state = self
            .owner
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if let Some(transfer) = state.transfers.remove(&self.id) {
            transfer.cancel();
        }
        drop(state);
        let _ = self.owner.list(None);
    }
}

struct Removal {
    owner: Arc<Registry>,
    id: String,
}
impl Drop for Removal {
    fn drop(&mut self) {
        let mut state = self
            .owner
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        state.removing.remove(&self.id);
        drop(state);
        let _ = self.owner.list(None);
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
        let path = directory.clone();
        let model = id.to_owned();
        let inspecting = definition.clone();
        let owner = self.clone();
        if tokio::task::spawn_blocking(move || {
            let mut state = owner
                .state
                .lock()
                .expect("local model configuration lock is healthy");
            inspect(&mut state, &path, &model, &inspecting)
        })
        .await
        .map_err(failure)?
        .is_some()
        {
            return Ok(());
        }
        let (stop, mirror) = {
            let mut state = self
                .state
                .lock()
                .expect("local model configuration lock is healthy");
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
            let resource = {
                let state = self
                    .state
                    .lock()
                    .expect("local model configuration lock is healthy");
                Resource {
                    id: id.into(),
                    definition: definition.clone(),
                    directory: directory.clone(),
                    stamp,
                    cancel: stop.clone(),
                    epoch: *state.epochs.get(id).unwrap_or(&0),
                }
            };
            let captured = resource.clone();
            let owner = self.clone();
            let check = Arc::new(move || owner.installing(&captured));
            let path = directory.clone();
            let definition = definition.clone();
            self.runtime
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
            if stop.is_cancelled() {
                return Err(cancelled());
            }
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
        let state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if !state.transfers.contains_key(&resource.id)
            || state.removing.contains(&resource.id)
            || state.definitions.get(&resource.id) != Some(&resource.definition)
            || *state.epochs.get(&resource.id).unwrap_or(&0) != resource.epoch
        {
            return Err(conflict());
        }
        Ok(())
    }

    pub(crate) async fn remove(self: &Arc<Self>, id: &str) -> Result<(), Fault> {
        let definition = self.definition(id)?;
        {
            let mut state = self
                .state
                .lock()
                .expect("local model configuration lock is healthy");
            state.removing.insert(id.into());
            let epoch = state.epochs.entry(id.into()).or_default();
            *epoch = epoch
                .checked_add(1)
                .ok_or_else(|| failure("local model epoch exhausted"))?;
            self.publish(&state);
        }
        let _guard = Removal {
            owner: self.clone(),
            id: id.into(),
        };
        self.runtime.unload(id).await?;
        let root = self.root()?;
        let model = id.to_owned();
        tokio::task::spawn_blocking(move || files::remove(&root, &model, &definition))
            .await
            .map_err(failure)?
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
