//! Model identities, verified installations and configuration owned by one Node.
pub(crate) mod catalog;
pub(super) mod files;
mod install;
#[cfg(test)]
mod tests;

use super::runtime::{Engine, Lane, Runtime};
pub(crate) use catalog::Registration;
use sailry_link::CancellationToken;
use sailry_model_assets::verified::Stamp;
use sailry_protocol::{
    ErrorCode, Fault,
    models::{Choice, Definition, Kind, Model, Settings, State},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

pub(crate) struct Registry {
    profile: Option<PathBuf>,
    state: Mutex<Configuration>,
    updates: watch::Sender<Vec<Model>>,
    mutations: BTreeMap<Kind, tokio::sync::Mutex<()>>,
    runtime: Runtime,
    stop: CancellationToken,
}

struct Configuration {
    settings: Settings,
    definitions: BTreeMap<String, Arc<Definition>>,
    installed: BTreeMap<String, Stamp>,
    epochs: BTreeMap<String, u64>,
    removing: BTreeSet<String>,
    transfers: BTreeMap<String, Transfer>,
    scopes: BTreeMap<Kind, CancellationToken>,
}

struct Transfer {
    cancel: CancellationToken,
    progress: sailry_model_assets::Progress,
}

#[derive(Clone, Debug)]
pub(crate) struct Resource {
    pub id: String,
    pub definition: Arc<Definition>,
    pub directory: PathBuf,
    pub stamp: Stamp,
    pub epoch: u64,
    pub(super) cancel: CancellationToken,
}

impl Registry {
    pub(crate) fn new(profile: Option<PathBuf>, stop: CancellationToken) -> Arc<Self> {
        let state = Configuration {
            settings: Settings::default(),
            definitions: catalog::definitions(&[]).expect("pinned local models are valid"),
            installed: BTreeMap::new(),
            epochs: BTreeMap::new(),
            removing: BTreeSet::new(),
            transfers: BTreeMap::new(),
            scopes: [Kind::Embedding, Kind::Ocr, Kind::Decision]
                .into_iter()
                .map(|kind| (kind, stop.child_token()))
                .collect(),
        };
        let (updates, _) = watch::channel(infos(&state));
        Arc::new(Self {
            profile,
            state: Mutex::new(state),
            updates,
            mutations: [Kind::Embedding, Kind::Ocr, Kind::Decision]
                .into_iter()
                .map(|kind| (kind, tokio::sync::Mutex::new(())))
                .collect(),
            runtime: Runtime::new(stop.child_token()),
            stop,
        })
    }

    pub(crate) fn configure(
        &self,
        mut settings: Settings,
        records: &[Registration],
    ) -> Result<(), Fault> {
        settings.fill_defaults();
        settings.validate()?;
        let mut definitions = catalog::definitions(records)?;
        for (kind, choice) in &settings.choices {
            if choice.model.as_ref().is_some_and(|id| {
                !definitions
                    .get(id)
                    .is_some_and(|definition| definition.format.kind() == *kind)
            }) {
                return Err(Fault::invalid(
                    "selected local model is not registered for its kind",
                ));
            }
        }
        let mut state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if state
            .definitions
            .iter()
            .any(|(id, previous)| definitions.get(id).is_some_and(|next| next != previous))
        {
            return Err(Fault::invalid("registered model definition is immutable"));
        }
        // Catalog reads can finish out of order. Registrations are immutable;
        // deleting an installation does not unregister its definition.
        for (id, definition) in &state.definitions {
            definitions
                .entry(id.clone())
                .or_insert_with(|| definition.clone());
        }
        if settings.revision < state.settings.revision {
            settings = state.settings.clone();
        }
        let changed = state.settings != settings || state.definitions != definitions;
        for (id, transfer) in &state.transfers {
            if source_changed(
                &definitions[id],
                state.settings.mirror.as_deref(),
                settings.mirror.as_deref(),
            ) {
                transfer.cancel.cancel();
            }
        }
        for kind in [Kind::Embedding, Kind::Ocr, Kind::Decision] {
            if state.settings.choice(kind) != settings.choice(kind) {
                let scope = state.scopes.get_mut(&kind).expect("Node model kind exists");
                scope.cancel();
                *scope = self.stop.child_token();
            }
        }
        state.settings = settings;
        state.definitions = definitions;
        if changed {
            self.updates.send_replace(infos(&state));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn settings(&self) -> Settings {
        self.state
            .lock()
            .expect("local model configuration lock is healthy")
            .settings
            .clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Vec<Model>> {
        self.updates.subscribe()
    }

    pub(crate) async fn models(self: &Arc<Self>, kind: Option<Kind>) -> Result<Vec<Model>, Fault> {
        if kind == Some(Kind::Speech) {
            return Err(Fault::invalid("speech belongs to the controller"));
        }
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            owner.refresh()?;
            Ok(owner
                .snapshot()
                .into_iter()
                .filter(|model| kind.is_none_or(|kind| model.definition.format.kind() == kind))
                .collect())
        })
        .await
        .map_err(failure)?
    }

    pub(crate) fn snapshot(&self) -> Vec<Model> {
        infos(
            &self
                .state
                .lock()
                .expect("local model configuration lock is healthy"),
        )
    }

    fn root(&self) -> Result<PathBuf, Fault> {
        files::root(self.profile.as_deref().ok_or_else(|| {
            Fault::new(ErrorCode::Unavailable, "local model storage is unavailable")
        })?)
    }

    fn refresh(&self) -> Result<(), Fault> {
        if self.profile.is_none() {
            return Ok(());
        }
        let root = self.root()?;
        let previous = {
            let state = self
                .state
                .lock()
                .expect("local model configuration lock is healthy");
            state
                .definitions
                .iter()
                .filter(|(id, _)| !state.removing.contains(*id))
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
                let directory = files::directory(&root, &id, &definition)?;
                let stamp = match &cached {
                    Some(cached)
                        if files::stamp(&directory, &id, &definition).as_ref() == Some(cached) =>
                    {
                        Some(cached.clone())
                    }
                    _ => files::receipt(&directory, &id, &definition),
                };
                Ok((id, cached, stamp))
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        let mut state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        let mut changed = false;
        for (id, previous, current) in observed {
            if state.definitions.contains_key(&id)
                && !state.removing.contains(&id)
                && state.installed.get(&id) == previous.as_ref()
                && previous != current
            {
                match current {
                    Some(stamp) => {
                        state.installed.insert(id.clone(), stamp);
                    }
                    None => {
                        state.installed.remove(&id);
                    }
                }
                let epoch = state.epochs.entry(id).or_default();
                *epoch = epoch
                    .checked_add(1)
                    .ok_or_else(|| failure("local model epoch exhausted"))?;
                changed = true;
            }
        }
        if changed {
            self.updates.send_replace(infos(&state));
        }
        Ok(())
    }

    pub(crate) fn definition(&self, id: &str) -> Result<Arc<Definition>, Fault> {
        self.state
            .lock()
            .expect("local model configuration lock is healthy")
            .definitions
            .get(id)
            .cloned()
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "local model is not registered"))
    }

    pub(crate) fn resource(
        &self,
        kind: Kind,
        id: Option<&str>,
        choice: Option<&Choice>,
    ) -> Result<Resource, Fault> {
        self.refresh()?;
        let root = self.root()?;
        let state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if choice.is_some_and(|choice| state.settings.choice(kind) != *choice) {
            return Err(conflict());
        }
        let choice = state.settings.choice(kind);
        if !choice.enabled && (kind != Kind::Embedding || id.is_none()) {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "local model service is disabled",
            ));
        }
        let id = id
            .or_else(|| {
                choice
                    .model
                    .as_deref()
                    .filter(|id| kind != Kind::Ocr || state.installed.contains_key(*id))
            })
            .or_else(|| {
                (kind == Kind::Ocr)
                    .then(|| {
                        catalog::ocr()
                            .iter()
                            .map(catalog::id)
                            .find(|id| state.installed.contains_key(id))
                            .or_else(|| {
                                state
                                    .installed
                                    .keys()
                                    .find(|id| {
                                        state.definitions.get(*id).is_some_and(|definition| {
                                            definition.format.kind() == Kind::Ocr
                                        })
                                    })
                                    .cloned()
                            })
                    })
                    .flatten()
                    .and_then(|id| {
                        state
                            .definitions
                            .get_key_value(&id)
                            .map(|(id, _)| id.as_str())
                    })
            })
            .ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "a local model must be selected")
            })?;
        let definition = state
            .definitions
            .get(id)
            .filter(|definition| definition.format.kind() == kind)
            .cloned()
            .ok_or_else(|| Fault::invalid("local model kind does not match the request"))?;
        let stamp = state
            .installed
            .get(id)
            .filter(|_| !state.removing.contains(id))
            .cloned()
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "local model is not installed"))?;
        Ok(Resource {
            id: id.into(),
            directory: files::directory(&root, id, &definition)?,
            definition,
            stamp,
            epoch: *state.epochs.get(id).unwrap_or(&0),
            cancel: state
                .scopes
                .get(&kind)
                .expect("Node model kind exists")
                .clone(),
        })
    }

    fn current(&self, resource: &Resource) -> Result<(), Fault> {
        if self.stop.is_cancelled() || resource.cancel.is_cancelled() {
            return Err(cancelled());
        }
        let state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if *state.epochs.get(&resource.id).unwrap_or(&0) != resource.epoch
            || state.removing.contains(&resource.id)
            || state.installed.get(&resource.id) != Some(&resource.stamp)
            || files::stamp(&resource.directory, &resource.id, &resource.definition).as_ref()
                != Some(&resource.stamp)
        {
            return Err(conflict());
        }
        Ok(())
    }

    pub(crate) async fn execute<T: Send + 'static>(
        self: &Arc<Self>,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        let owner = self.clone();
        let captured = resource.clone();
        self.runtime
            .execute(
                resource,
                lane,
                cancel,
                Arc::new(move || owner.current(&captured)),
                operation,
            )
            .await
    }

    pub(crate) fn blocking<T: Send + 'static>(
        self: &Arc<Self>,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        let owner = self.clone();
        let captured = resource.clone();
        self.runtime.blocking(
            resource,
            lane,
            cancel,
            Arc::new(move || owner.current(&captured)),
            operation,
        )
    }

    pub(crate) fn signature(&self, kind: Kind) -> [u8; 32] {
        self.resource(kind, None, None).map_or([0; 32], |resource| {
            *blake3::hash(resource.id.as_bytes()).as_bytes()
        })
    }

    pub(crate) fn cancel(&self, id: &str) -> bool {
        let state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        state.transfers.get(id).is_some_and(|transfer| {
            transfer.cancel.cancel();
            true
        })
    }

    pub(crate) async fn shutdown(&self) {
        self.stop.cancel();
        self.runtime.shutdown().await;
    }
}

fn infos(state: &Configuration) -> Vec<Model> {
    state
        .definitions
        .iter()
        .map(|(id, definition)| Model {
            id: id.clone(),
            definition: definition.as_ref().clone(),
            state: if let Some(transfer) = state.transfers.get(id) {
                State::Downloading {
                    percent: transfer.progress.percent.clamp(0., 99.) as u8,
                    bytes_per_second: transfer.progress.bytes_per_second.max(0.) as u64,
                }
            } else if state.installed.contains_key(id) && !state.removing.contains(id) {
                State::Installed
            } else {
                State::Missing
            },
        })
        .collect()
}

fn source_changed(definition: &Definition, previous: Option<&str>, current: Option<&str>) -> bool {
    previous != current
        && definition.assets.iter().any(|asset| {
            sailry_model_assets::url(asset, previous) != sailry_model_assets::url(asset, current)
        })
}

pub(crate) fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("local model failed: {error}"),
    )
}
pub(crate) fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "local model operation was cancelled")
}
fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "local model configuration or assets changed",
    )
}
