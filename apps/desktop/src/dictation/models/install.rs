use super::*;
use sailry_model_assets::Download;

impl Manager {
    pub fn start(
        self: &Arc<Self>,
        id: Option<ModelId>,
        mirror: Option<String>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<CancellationToken> {
        let definition = {
            let mut data = self.data.lock().unwrap();
            if data.active.contains_key(&id) {
                return Err("dictation_busy");
            }
            if let Some(job) = data.jobs.get(&id)
                && matches!(job.state, State::Downloading(_))
            {
                return Ok(job.cancel.clone());
            }
            let definition = id
                .map(|id| {
                    data.records
                        .iter()
                        .find(|record| record.id == id)
                        .map(|record| record.definition.clone())
                        .ok_or("dictation_model_missing")
                })
                .transpose()?;
            data.jobs.insert(
                id,
                Job {
                    cancel: CancellationToken::new(),
                    state: State::Downloading(Progress::default()),
                },
            );
            definition
        };
        let cancel = self.data.lock().unwrap().jobs[&id].cancel.clone();
        let stopping = cancel.clone();
        let manager = self.clone();
        self.changed();
        runtime.spawn(async move {
            let path = manager.path(id);
            let update = |progress| {
                manager.progress(id, progress);
            };
            let result = if let Some(definition) = definition {
                let downloads = definition
                    .assets
                    .iter()
                    .map(|asset| {
                        Ok(Download {
                            file: File {
                                name: &asset.name,
                                size: asset.bytes,
                                sha256: &asset.sha256,
                            },
                            url: sailry_model_assets::url(asset, mirror.as_deref())
                                .map_err(|_| "dictation_model_invalid")?,
                        })
                    })
                    .collect::<Result<Vec<_>>>();
                match downloads {
                    Ok(downloads) => {
                        sailry_model_assets::prepare(&path, &downloads, &stopping, update)
                            .await
                            .map_err(asset_error)
                    }
                    Err(error) => Err(error),
                }
            } else {
                model::prepare_with_progress(&path, &stopping, update)
                    .await
                    .map_err(error_key)
            };
            let result = match result {
                Ok(_) if !stopping.is_cancelled() => {
                    let graph = path.join(if id.is_some() {
                        "model.onnx"
                    } else {
                        "model.int8.onnx"
                    });
                    let tokens = path.join("tokens.txt");
                    tokio::task::spawn_blocking(move || validation::check(&graph, &tokens))
                        .await
                        .unwrap_or(Err("dictation_model_invalid"))
                }
                Ok(_) => Err("dictation_cancelled"),
                Err(error) => Err(error),
            };
            manager.finish(id, result, &stopping);
        });
        Ok(cancel)
    }

    fn progress(&self, id: Option<ModelId>, progress: Progress) {
        if let Some(job) = self.data.lock().unwrap().jobs.get_mut(&id) {
            job.state = State::Downloading(progress);
        }
        self.changed();
    }

    pub(super) fn finish(
        &self,
        id: Option<ModelId>,
        mut result: Result<()>,
        cancel: &CancellationToken,
    ) {
        let mut data = self.data.lock().unwrap();
        if cancel.is_cancelled() {
            result = Err("dictation_cancelled");
        }
        if let Some(id) = id
            && result.is_ok()
        {
            let record = data
                .records
                .iter_mut()
                .find(|record| record.id == id)
                .unwrap();
            let previous = record.installed;
            record.installed = true;
            if let Err(error) = self.save(&data.records) {
                data.records
                    .iter_mut()
                    .find(|record| record.id == id)
                    .unwrap()
                    .installed = previous;
                result = Err(error);
            }
        }
        if let Some(job) = data.jobs.get_mut(&id) {
            job.state = match result {
                Ok(()) => State::Installed,
                Err("dictation_cancelled") => State::Missing,
                Err(error) => State::Failed(error),
            };
        }
        self.changed();
    }
}
