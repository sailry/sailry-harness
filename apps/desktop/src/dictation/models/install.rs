use super::*;
use sailry_model_assets::Download;

impl Manager {
    pub fn start(
        self: &Arc<Self>,
        id: Option<ModelId>,
        mirror: Option<String>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<CancellationToken> {
        let request = {
            let data = self.data.lock().unwrap();
            data.jobs
                .get(&id)
                .filter(|job| matches!(job.state, State::Downloading(_)))
                .and_then(|_| data.requests.get(&id).copied())
                .unwrap_or_else(RequestId::new)
        };
        self.start_request(id, mirror, request, runtime)
    }

    pub fn start_request(
        self: &Arc<Self>,
        id: Option<ModelId>,
        mirror: Option<String>,
        request: RequestId,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<CancellationToken> {
        let definition = {
            let mut data = self.data.lock().unwrap();
            if data.outcomes.contains_key(&request) {
                return Ok(CancellationToken::new());
            }
            if data.active.contains_key(&id) {
                return Err("dictation_busy");
            }
            if let Some(job) = data.jobs.get(&id)
                && matches!(job.state, State::Downloading(_))
            {
                if data.requests.get(&id) != Some(&request) {
                    return Err("dictation_busy");
                }
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
            data.requests.insert(id, request);
            data.installations.insert(
                request,
                sailry_protocol::installation::Task {
                    request,
                    revision: 1,
                    resource: sailry_protocol::installation::ModelRef::Speech {
                        target: sailry_protocol::installation::Target::Controller,
                        id,
                    }
                    .resource(),
                    state: sailry_protocol::installation::State::Queued,
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
                    manager.progress(
                        id,
                        Progress {
                            phase: sailry_model_assets::Phase::Validating,
                            ..Default::default()
                        },
                    );
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

    pub(super) fn progress(&self, id: Option<ModelId>, progress: Progress) {
        let mut data = self.data.lock().unwrap();
        if let Some(job) = data.jobs.get_mut(&id) {
            job.state = State::Downloading(progress);
        }
        if let Some(request) = data.requests.get(&id).copied()
            && let Some(task) = data.installations.get_mut(&request)
        {
            use sailry_protocol::installation::{Phase, Progress as Measurement, State};
            task.revision += 1;
            task.state = State::Running {
                phase: match progress.phase {
                    sailry_model_assets::Phase::Inspecting => Phase::Inspecting,
                    sailry_model_assets::Phase::Downloading => Phase::Downloading,
                    sailry_model_assets::Phase::Validating => Phase::Validating,
                    sailry_model_assets::Phase::Installing => Phase::Installing,
                },
                progress: (progress.phase == sailry_model_assets::Phase::Downloading).then_some(
                    Measurement::Percent {
                        percent: progress.percent.clamp(0., 100.) as u8,
                        bytes_per_second: Some(progress.bytes_per_second.max(0.) as u64),
                    },
                ),
            };
        }
        drop(data);
        self.changed();
    }

    pub(super) fn finish(
        &self,
        id: Option<ModelId>,
        mut result: Result<()>,
        cancel: &CancellationToken,
    ) {
        if id.is_some() && result.is_ok() && !cancel.is_cancelled() {
            self.progress(
                id,
                Progress {
                    phase: sailry_model_assets::Phase::Installing,
                    ..Default::default()
                },
            );
        }
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
        if let Some(request) = data.requests.get(&id).copied() {
            data.outcomes.insert(request, result);
            if let Err(error) = result {
                if let Some(task) = data.installations.get_mut(&request) {
                    task.revision += 1;
                    task.state = sailry_protocol::installation::State::Failed {
                        error: sailry_protocol::Fault::new(
                            if error == "dictation_cancelled" {
                                sailry_protocol::ErrorCode::Cancelled
                            } else {
                                sailry_protocol::ErrorCode::Internal
                            },
                            error,
                        ),
                    };
                }
            } else {
                data.installations.remove(&request);
            }
        }
        drop(data);
        self.changed();
    }
}
