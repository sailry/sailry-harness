//! Controller-owned speech installations; remote Nodes only resolve public model metadata.
mod install;
#[cfg(test)]
pub(super) mod tests;
mod validation;

use super::{Result, error_key, model};
use sailry_link::CancellationToken;
use sailry_model_assets::{File, Progress};
use sailry_protocol::{
    ModelId, RequestId,
    models::{Definition, Format},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: ModelId,
    pub definition: Definition,
    #[serde(default)]
    pub installed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum State {
    Missing,
    Downloading(Progress),
    Installed,
    Failed(&'static str),
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub record: Record,
    pub state: State,
}

#[derive(Default)]
struct Data {
    records: Vec<Record>,
    jobs: BTreeMap<Option<ModelId>, Job>,
    active: BTreeMap<Option<ModelId>, usize>,
    installations: BTreeMap<RequestId, sailry_protocol::installation::Task>,
    requests: BTreeMap<Option<ModelId>, RequestId>,
    outcomes: BTreeMap<RequestId, Result<()>>,
}

struct Job {
    cancel: CancellationToken,
    state: State,
}

pub(crate) struct Manager {
    directory: PathBuf,
    data: Mutex<Data>,
    changes: tokio::sync::watch::Sender<u64>,
}

#[derive(Clone)]
pub(crate) struct Capture {
    directory: PathBuf,
    definition: Option<Definition>,
    _lease: Option<Arc<Lease>>,
}

struct Lease {
    manager: Arc<Manager>,
    id: Option<ModelId>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut data = self.manager.data.lock().unwrap();
        if let Some(count) = data.active.get_mut(&self.id) {
            *count -= 1;
            if *count == 0 {
                data.active.remove(&self.id);
            }
        }
    }
}

impl Capture {
    pub fn ready(&self) -> bool {
        if let Some(definition) = &self.definition {
            sailry_model_assets::ready(&self.directory, &files(definition))
        } else {
            model::ready(&self.directory)
        }
    }

    pub fn recognizer(&self, language: sailry_speech::Language) -> Result<model::Recognizer> {
        if self.definition.is_some() {
            model::Recognizer::from_paths(
                &self.directory.join("model.onnx"),
                &self.directory.join("tokens.txt"),
                language,
            )
            .map_err(error_key)
        } else {
            model::Recognizer::new(&self.directory, language).map_err(error_key)
        }
    }

    #[cfg(test)]
    pub fn default_at(directory: PathBuf) -> Self {
        Self {
            directory,
            definition: None,
            _lease: None,
        }
    }
}

impl Manager {
    pub fn installations(&self) -> Vec<sailry_protocol::installation::Task> {
        self.data
            .lock()
            .unwrap()
            .installations
            .values()
            .cloned()
            .collect()
    }

    pub fn outcome(&self, request: RequestId) -> Option<Result<()>> {
        self.data.lock().unwrap().outcomes.get(&request).copied()
    }

    pub fn dismiss(&self, request: RequestId) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        if data
            .installations
            .get(&request)
            .is_some_and(|task| !task.state.terminal())
        {
            return Err("dictation_busy");
        }
        data.installations.remove(&request);
        drop(data);
        self.changed();
        Ok(())
    }
    pub fn open(directory: PathBuf) -> Result<Arc<Self>> {
        let records: Vec<Record> = match std::fs::read(directory.join("models/speech.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "dictation_storage")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("dictation_storage"),
        };
        let mut ids = std::collections::BTreeSet::new();
        for record in &records {
            record
                .definition
                .validate()
                .map_err(|_| "dictation_storage")?;
            if record.definition.format != (Format::SenseVoice {}) || !ids.insert(record.id) {
                return Err("dictation_storage");
            }
        }
        let (changes, _) = tokio::sync::watch::channel(0);
        Ok(Arc::new(Self {
            directory,
            data: Mutex::new(Data {
                records,
                ..Default::default()
            }),
            changes,
        }))
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }
    fn changed(&self) {
        self.changes
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    pub fn entries(&self) -> Vec<Entry> {
        let data = self.data.lock().unwrap();
        data.records
            .iter()
            .map(|record| Entry {
                record: record.clone(),
                state: data.jobs.get(&Some(record.id)).map_or(
                    if record.installed {
                        State::Installed
                    } else {
                        State::Missing
                    },
                    |job| job.state,
                ),
            })
            .collect()
    }

    pub fn default_state(&self) -> Option<State> {
        self.data
            .lock()
            .unwrap()
            .jobs
            .get(&None)
            .map(|job| job.state)
    }

    pub fn observe(&self, id: Option<ModelId>, ready: bool) {
        let mut data = self.data.lock().unwrap();
        if data
            .jobs
            .get(&id)
            .is_some_and(|job| matches!(job.state, State::Downloading(_)))
        {
            return;
        }
        let state = if ready {
            State::Installed
        } else {
            State::Missing
        };
        if data.jobs.get(&id).is_some_and(|job| job.state == state) {
            return;
        }
        data.jobs.insert(
            id,
            Job {
                cancel: CancellationToken::new(),
                state,
            },
        );
        self.changed();
    }

    pub fn capture(self: &Arc<Self>, id: Option<ModelId>) -> Result<Capture> {
        let mut data = self.data.lock().unwrap();
        if data
            .jobs
            .get(&id)
            .is_some_and(|job| matches!(job.state, State::Downloading(_)))
        {
            return Err("dictation_model_missing");
        }
        let definition = if let Some(id) = id {
            let record = data
                .records
                .iter()
                .find(|record| record.id == id && record.installed)
                .ok_or("dictation_model_missing")?;
            Some(record.definition.clone())
        } else {
            None
        };
        *data.active.entry(id).or_default() += 1;
        Ok(Capture {
            directory: self.path(id),
            definition,
            _lease: Some(Arc::new(Lease {
                manager: self.clone(),
                id,
            })),
        })
    }

    pub fn register(&self, definition: Definition) -> Result<ModelId> {
        definition
            .validate()
            .map_err(|_| "dictation_model_invalid")?;
        if definition.format != (Format::SenseVoice {}) {
            return Err("dictation_model_invalid");
        }
        let mut data = self.data.lock().unwrap();
        if let Some(record) = data
            .records
            .iter()
            .find(|record| record.definition.source == definition.source)
        {
            return if record.definition == definition {
                Ok(record.id)
            } else {
                Err("model_library_conflict")
            };
        }
        let id = ModelId::new();
        data.records.push(Record {
            id,
            definition,
            installed: false,
        });
        if let Err(error) = self.save(&data.records) {
            data.records.pop();
            return Err(error);
        }
        self.changed();
        Ok(id)
    }

    pub fn cancelling(&self, id: Option<ModelId>) -> bool {
        self.data
            .lock()
            .unwrap()
            .jobs
            .get(&id)
            .is_some_and(|job| job.cancel.is_cancelled())
    }

    pub fn cancel(&self, id: Option<ModelId>) {
        if let Some(job) = self.data.lock().unwrap().jobs.get(&id) {
            job.cancel.cancel();
        }
    }

    pub fn remove(&self, id: ModelId) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        if data.active.contains_key(&Some(id))
            || data
                .jobs
                .get(&Some(id))
                .is_some_and(|job| matches!(job.state, State::Downloading(_)))
        {
            return Err("dictation_busy");
        }
        let index = data
            .records
            .iter()
            .position(|record| record.id == id)
            .ok_or("dictation_model_missing")?;
        let path = self.path(Some(id));
        // Keep the installation recoverable until its controller index is saved.
        let temporary = if path.exists() {
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| "dictation_storage")?;
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err("dictation_storage");
            }
            let temporary = tempfile::Builder::new()
                .prefix(".speech-delete-")
                .tempdir_in(path.parent().ok_or("dictation_storage")?)
                .map_err(|_| "dictation_storage")?;
            std::fs::rename(&path, temporary.path().join("model"))
                .map_err(|_| "dictation_storage")?;
            Some(temporary)
        } else {
            None
        };
        let record = data.records.remove(index);
        if let Err(error) = self.save(&data.records) {
            data.records.insert(index, record);
            if let Some(temporary) = temporary
                && std::fs::rename(temporary.path().join("model"), &path).is_err()
            {
                // Preserve the files for recovery if an external writer blocks restoration.
                let _ = temporary.keep();
            }
            return Err(error);
        }
        drop(temporary);
        data.jobs.remove(&Some(id));
        self.changed();
        Ok(())
    }

    fn path(&self, id: Option<ModelId>) -> PathBuf {
        match id {
            Some(id) => self.directory.join("models/speech").join(id.to_string()),
            None => self.directory.join("models/sensevoice-int8"),
        }
    }

    fn save(&self, records: &[Record]) -> Result<()> {
        let directory = self.directory.join("models");
        std::fs::create_dir_all(&directory).map_err(|_| "dictation_storage")?;
        let mut file =
            tempfile::NamedTempFile::new_in(&directory).map_err(|_| "dictation_storage")?;
        file.write_all(&serde_json::to_vec(records).map_err(|_| "dictation_storage")?)
            .map_err(|_| "dictation_storage")?;
        file.as_file().sync_all().map_err(|_| "dictation_storage")?;
        file.persist(directory.join("speech.json"))
            .map_err(|_| "dictation_storage")?;
        Ok(())
    }
}

fn files(definition: &Definition) -> Vec<File<'_>> {
    definition
        .assets
        .iter()
        .map(|asset| File {
            name: &asset.name,
            size: asset.bytes,
            sha256: &asset.sha256,
        })
        .collect()
}

fn asset_error(error: sailry_model_assets::Error) -> &'static str {
    match error {
        sailry_model_assets::Error::Storage(_) => "dictation_storage",
        sailry_model_assets::Error::Download(_) => "dictation_download",
        sailry_model_assets::Error::InvalidAsset => "dictation_model_invalid",
        sailry_model_assets::Error::Cancelled => "dictation_cancelled",
    }
}
