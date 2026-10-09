//! One lazy, bounded OCR owner per execution Node.
pub(crate) mod catalog;
pub(crate) mod document;
mod files;
mod install;
#[cfg(test)]
mod tests;

use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    models::Definition,
    ocr::{Model, Settings, State},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, watch};

const IDLE: Duration = Duration::from_secs(300);

pub(crate) struct Service {
    profile: Option<PathBuf>,
    state: Mutex<Configuration>,
    engine: Mutex<Option<Loaded>>,
    updates: watch::Sender<Vec<Model>>,
    finished: Notify,
    mutation: tokio::sync::Mutex<()>,
    stop: CancellationToken,
}

struct Configuration {
    settings: Settings,
    definitions: BTreeMap<String, Arc<Definition>>,
    installed: BTreeMap<String, files::Stamp>,
    transfers: BTreeMap<String, Transfer>,
}

struct Transfer {
    cancel: CancellationToken,
    progress: sailry_model_assets::Progress,
}
struct Loaded {
    id: String,
    stamp: files::Stamp,
    used: Instant,
    engine: sailry_ocr::Engine,
}

impl Service {
    pub(crate) fn new(profile: Option<PathBuf>, stop: CancellationToken) -> Arc<Self> {
        let state = Configuration {
            settings: Settings::default(),
            definitions: catalog::definitions(&[]).expect("OCR catalog is valid"),
            installed: BTreeMap::new(),
            transfers: BTreeMap::new(),
        };
        let (updates, _) = watch::channel(infos(&state));
        let owner = Arc::new(Self {
            profile,
            state: Mutex::new(state),
            engine: Mutex::new(None),
            updates,
            finished: Notify::new(),
            mutation: tokio::sync::Mutex::new(()),
            stop,
        });
        let weak = Arc::downgrade(&owner);
        let stopping = owner.stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! { _ = stopping.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(30)) => () }
                let Some(owner) = weak.upgrade() else { break };
                if let Ok(mut loaded) = owner.engine.try_lock()
                    && loaded
                        .as_ref()
                        .is_some_and(|entry| entry.used.elapsed() >= IDLE)
                {
                    *loaded = None;
                }
            }
        });
        owner
    }

    pub(crate) fn configure(
        &self,
        settings: Settings,
        records: &[Definition],
    ) -> Result<(), Fault> {
        settings.validate()?;
        let definitions = catalog::definitions(records)?;
        if settings
            .model
            .as_ref()
            .is_some_and(|id| !definitions.contains_key(id))
        {
            return Err(Fault::invalid("OCR model is not registered"));
        }
        let mut state = self.state.lock().expect("OCR configuration lock is valid");
        state.installed.retain(|id, _| definitions.contains_key(id));
        state.settings = settings;
        state.definitions = definitions;
        self.updates.send_replace(infos(&state));
        Ok(())
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Vec<Model>> {
        self.updates.subscribe()
    }

    pub(crate) async fn models(self: &Arc<Self>) -> Result<Vec<Model>, Fault> {
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            owner.refresh()?;
            Ok(owner.snapshot())
        })
        .await
        .map_err(failure)?
    }

    fn snapshot(&self) -> Vec<Model> {
        infos(&self.state.lock().expect("OCR configuration lock is valid"))
    }

    fn root(&self) -> Result<PathBuf, Fault> {
        files::open(self.profile.as_deref().ok_or_else(unavailable)?)
    }

    fn refresh(&self) -> Result<(), Fault> {
        let root = self.root()?;
        let previous = {
            let state = self.state.lock().expect("OCR configuration lock is valid");
            state
                .definitions
                .iter()
                .map(|(id, definition)| {
                    (
                        id.clone(),
                        definition.clone(),
                        state.installed.get(id).cloned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let observed = previous
            .into_iter()
            .map(|(id, definition, cached)| {
                let directory = root.join(&id);
                let stamp = match &cached {
                    Some(cached)
                        if files::stamp(&directory, &definition).as_ref() == Some(cached) =>
                    {
                        Some(cached.clone())
                    }
                    _ => files::verified(&directory, &definition),
                };
                (id, cached, stamp)
            })
            .collect::<Vec<_>>();
        let mut state = self.state.lock().expect("OCR configuration lock is valid");
        let mut changed = false;
        for (id, previous, observed) in observed {
            if state.definitions.contains_key(&id)
                && state.installed.get(&id) == previous.as_ref()
                && previous != observed
            {
                match observed {
                    Some(stamp) => {
                        state.installed.insert(id, stamp);
                    }
                    None => {
                        state.installed.remove(&id);
                    }
                }
                changed = true;
            }
        }
        if changed {
            self.updates.send_replace(infos(&state));
        }
        Ok(())
    }

    pub(crate) fn recognize(
        &self,
        image: &image::RgbImage,
        cancel: &CancellationToken,
    ) -> Result<Vec<sailry_ocr::Line>, Fault> {
        self.refresh()?;
        let (id, definition, stamp) = {
            let state = self.state.lock().expect("OCR configuration lock is valid");
            if !state.settings.enabled {
                return Err(Fault::new(ErrorCode::NotConfigured, "OCR is disabled"));
            }
            let id = state
                .settings
                .model
                .as_ref()
                .filter(|id| state.installed.contains_key(*id))
                .cloned()
                .or_else(|| {
                    catalog::recommended()
                        .iter()
                        .map(catalog::id)
                        .find(|id| state.installed.contains_key(id))
                })
                .or_else(|| state.installed.keys().next().cloned())
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "an OCR model must be installed")
                })?;
            (
                id.clone(),
                state.definitions[&id].clone(),
                state.installed[&id].clone(),
            )
        };
        let mut loaded = self.engine.lock().expect("OCR inference lock is valid");
        if cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        if loaded
            .as_ref()
            .is_none_or(|entry| entry.id != id || entry.stamp != stamp)
        {
            *loaded = Some(Loaded {
                id: id.clone(),
                stamp,
                used: Instant::now(),
                engine: sailry_ocr::Engine::load(&definition, &self.root()?.join(&id))
                    .map_err(failure)?,
            });
        }
        let entry = loaded.as_mut().expect("OCR engine is prepared");
        entry.used = Instant::now();
        entry
            .engine
            .recognize(image, || cancel.is_cancelled() || self.stop.is_cancelled())
            .map_err(|error| match error {
                sailry_ocr::Error::Cancelled => cancelled(),
                error => failure(error),
            })
    }

    pub(crate) fn cancel(&self, id: &str) {
        if let Some(transfer) = self
            .state
            .lock()
            .expect("OCR configuration lock is valid")
            .transfers
            .get(id)
        {
            transfer.cancel.cancel();
        }
    }

    pub(crate) fn signature(&self) -> [u8; 32] {
        if self.profile.is_some() && self.refresh().is_err() {
            return [0; 32];
        }
        let state = self.state.lock().expect("OCR configuration lock is valid");
        if !state.settings.enabled {
            return [0; 32];
        }
        let id = state
            .settings
            .model
            .as_ref()
            .filter(|id| state.installed.contains_key(*id))
            .cloned()
            .or_else(|| {
                catalog::recommended()
                    .iter()
                    .map(catalog::id)
                    .find(|id| state.installed.contains_key(id))
            })
            .or_else(|| state.installed.keys().next().cloned());
        id.map_or([0; 32], |id| *blake3::hash(id.as_bytes()).as_bytes())
    }

    pub(crate) fn shutdown(&self) {
        self.stop.cancel();
        for transfer in self
            .state
            .lock()
            .expect("OCR configuration lock is valid")
            .transfers
            .values()
        {
            transfer.cancel.cancel();
        }
    }
}

fn infos(state: &Configuration) -> Vec<Model> {
    state
        .definitions
        .iter()
        .map(|(id, definition)| Model {
            id: id.clone(),
            definition: definition.as_ref().clone(),
            state: match state.transfers.get(id) {
                Some(transfer) => State::Downloading {
                    percent: transfer.progress.percent.clamp(0.0, 100.0) as u8,
                    bytes_per_second: transfer.progress.bytes_per_second.max(0.0) as u64,
                },
                None if state.installed.contains_key(id) => State::Installed,
                None => State::Missing,
            },
        })
        .collect()
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "OCR service is unavailable")
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "OCR operation was cancelled")
}
fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, format!("OCR failed: {error}"))
}
