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
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::watch;

pub(crate) struct Registry {
    profile: Option<PathBuf>,
    root: OnceLock<PathBuf>,
    state: Mutex<Configuration>,
    /// Serializes file inspection so concurrent listings do not hash the same files.
    inspecting: Mutex<()>,
    updates: watch::Sender<Update>,
    runtime: Runtime,
    handle: tokio::runtime::Handle,
    stop: CancellationToken,
}

/// Verified stamps are the installed-state authority. Removal clears a stamp before
/// deleting files, so queued captures fail without a separate generation counter.
struct Configuration {
    settings: Settings,
    definitions: BTreeMap<String, Arc<Definition>>,
    verified: BTreeMap<String, Stamp>,
    removing: BTreeSet<String>,
    transfers: BTreeMap<String, CancellationToken>,
    scopes: BTreeMap<Kind, CancellationToken>,
}

pub(crate) struct Prepared {
    settings: Settings,
    definitions: BTreeMap<String, Arc<Definition>>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Update {
    pub models: Vec<Model>,
    pub embedding: Choice,
    pub ocr: Choice,
}

#[derive(Clone, Debug)]
pub(crate) struct Resource {
    pub id: String,
    pub definition: Arc<Definition>,
    pub directory: PathBuf,
    pub stamp: Stamp,
    pub(super) cancel: CancellationToken,
}

impl Registry {
    pub(crate) fn new(profile: Option<PathBuf>, stop: CancellationToken) -> Arc<Self> {
        let state = Configuration {
            settings: Settings::default(),
            definitions: catalog::definitions(&[]).expect("pinned local models are valid"),
            verified: BTreeMap::new(),
            removing: BTreeSet::new(),
            transfers: BTreeMap::new(),
            scopes: Kind::NODE
                .into_iter()
                .map(|kind| (kind, stop.child_token()))
                .collect(),
        };
        let (updates, _) = watch::channel(update(&state));
        Arc::new(Self {
            profile,
            root: OnceLock::new(),
            state: Mutex::new(state),
            inspecting: Mutex::new(()),
            updates,
            runtime: Runtime::new(stop.child_token()),
            handle: tokio::runtime::Handle::current(),
            stop,
        })
    }

    pub(crate) fn prepare(
        &self,
        mut settings: Settings,
        definitions: BTreeMap<String, Arc<Definition>>,
    ) -> Result<Prepared, Fault> {
        settings.fill_defaults();
        settings.validate()?;
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
        let state = self
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
        Ok(Prepared {
            settings,
            definitions,
        })
    }

    pub(crate) fn apply(self: &Arc<Self>, prepared: Prepared) {
        let Prepared {
            settings,
            definitions,
        } = prepared;
        let mut state = self.lock();
        for (id, transfer) in &state.transfers {
            if source_changed(
                &definitions[id],
                state.settings.mirror.as_deref(),
                settings.mirror.as_deref(),
            ) {
                transfer.cancel();
            }
        }
        for kind in Kind::NODE {
            if state.settings.choice(kind) != settings.choice(kind) {
                let scope = state.scopes.get_mut(&kind).expect("Node model kind exists");
                scope.cancel();
                *scope = self.stop.child_token();
            }
        }
        let added = definitions
            .keys()
            .any(|id| !state.definitions.contains_key(id));
        state.verified.retain(|id, _| definitions.contains_key(id));
        state.settings = settings;
        state.definitions = definitions;
        self.publish(&state);
        drop(state);
        if added {
            // Registered files may already exist; inspect them off the caller's thread.
            let owner = self.clone();
            drop(self.handle.spawn_blocking(move || owner.list(None)));
        }
    }

    #[cfg(test)]
    pub(crate) fn configure(
        self: &Arc<Self>,
        settings: Settings,
        records: &[Registration],
    ) -> Result<(), Fault> {
        self.apply(self.prepare(settings, catalog::definitions(records)?)?);
        Ok(())
    }

    pub(crate) fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Configuration> {
        self.state
            .lock()
            .expect("local model configuration lock is healthy")
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Update> {
        self.updates.subscribe()
    }

    pub(crate) async fn models(self: &Arc<Self>, kind: Option<Kind>) -> Result<Vec<Model>, Fault> {
        let owner = self.clone();
        tokio::task::spawn_blocking(move || owner.list(kind))
            .await
            .map_err(failure)?
    }

    pub(crate) fn list(&self, kind: Option<Kind>) -> Result<Vec<Model>, Fault> {
        if kind == Some(Kind::Speech) {
            return Err(Fault::invalid("speech belongs to the controller"));
        }
        if self.profile.is_some() {
            self.verify(|_, definition| kind.is_none_or(|kind| definition.format.kind() == kind))?;
        }
        Ok(self.cached(kind))
    }

    /// Verifies files outside the configuration lock; hashing runs only for changed files.
    fn verify(&self, include: impl Fn(&str, &Definition) -> bool) -> Result<(), Fault> {
        let root = self.root()?;
        let _inspecting = self
            .inspecting
            .lock()
            .expect("local model inspection lock is healthy");
        let pending = {
            let state = self.lock();
            state
                .definitions
                .iter()
                .filter(|(id, definition)| include(id, definition) && !state.removing.contains(*id))
                .map(|(id, definition)| {
                    (
                        id.clone(),
                        definition.clone(),
                        state.verified.get(id).cloned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut observed = Vec::with_capacity(pending.len());
        for (id, definition, cached) in pending {
            let directory = files::directory(&root, &id, &definition)?;
            let stamp = files::stamp(&directory, &id, &definition, cached.as_ref());
            observed.push((id, definition, cached, stamp));
        }
        let mut state = self.lock();
        for (id, definition, cached, stamp) in observed {
            commit(&mut state, &id, &definition, cached.as_ref(), stamp);
        }
        self.publish(&state);
        Ok(())
    }

    /// Published models from verified stamps, without file access.
    pub(crate) fn cached(&self, kind: Option<Kind>) -> Vec<Model> {
        infos(&self.lock())
            .into_iter()
            .filter(|model| kind.is_none_or(|kind| model.definition.format.kind() == kind))
            .collect()
    }

    fn publish(&self, state: &Configuration) {
        let latest = update(state);
        self.updates.send_if_modified(|previous| {
            if *previous == latest {
                return false;
            }
            *previous = latest;
            true
        });
    }

    /// Reports whether model storage is usable, without inspecting model files.
    pub(crate) fn storage(&self) -> Result<(), Fault> {
        self.root().map(|_| ())
    }

    fn root(&self) -> Result<PathBuf, Fault> {
        if let Some(root) = self.root.get() {
            return Ok(root.clone());
        }
        let root = files::root(self.profile.as_deref().ok_or_else(|| {
            Fault::new(ErrorCode::Unavailable, "local model storage is unavailable")
        })?)?;
        Ok(self.root.get_or_init(|| root).clone())
    }

    pub(crate) fn definition(&self, id: &str) -> Result<Arc<Definition>, Fault> {
        self.lock()
            .definitions
            .get(id)
            .cloned()
            .ok_or_else(|| Fault::new(ErrorCode::NotFound, "local model is not registered"))
    }

    /// Captures a verified model. Executions check file identity once before running.
    pub(crate) fn resource(
        &self,
        kind: Kind,
        id: &str,
        choice: Option<&Choice>,
    ) -> Result<Resource, Fault> {
        let root = self.root()?;
        let (definition, cached, cancel) = {
            let state = self.lock();
            if choice.is_some_and(|choice| state.settings.choice(kind) != *choice) {
                return Err(conflict());
            }
            let definition = state
                .definitions
                .get(id)
                .filter(|definition| definition.format.kind() == kind)
                .cloned()
                .ok_or_else(|| Fault::invalid("local model kind does not match the request"))?;
            if state.removing.contains(id) {
                return Err(missing());
            }
            let cancel = state
                .scopes
                .get(&kind)
                .expect("Node model kind exists")
                .clone();
            (definition, state.verified.get(id).cloned(), cancel)
        };
        let directory = files::directory(&root, id, &definition)?;
        let stamp = match cached {
            Some(stamp) => stamp,
            None => {
                // Uncached files are accepted through their receipt; hashing stays with listings.
                let stamp = files::known(&directory, &definition, None).ok_or_else(missing)?;
                let mut state = self.lock();
                if commit(&mut state, id, &definition, None, Some(stamp.clone())) {
                    self.publish(&state);
                    stamp
                } else if state.removing.contains(id)
                    || state.definitions.get(id) != Some(&definition)
                {
                    return Err(conflict());
                } else {
                    // A concurrent listing or installation recorded the current stamp first.
                    state.verified.get(id).cloned().ok_or_else(missing)?
                }
            }
        };
        Ok(Resource {
            id: id.into(),
            directory,
            definition,
            stamp,
            cancel,
        })
    }

    fn current(&self, resource: &Resource, files: bool) -> Result<(), Fault> {
        if self.stop.is_cancelled() || resource.cancel.is_cancelled() {
            return Err(cancelled());
        }
        {
            let state = self.lock();
            if state.removing.contains(&resource.id)
                || state.definitions.get(&resource.id) != Some(&resource.definition)
                || state.verified.get(&resource.id) != Some(&resource.stamp)
            {
                return Err(conflict());
            }
        }
        if files && !files::unchanged(resource) {
            let mut state = self.lock();
            if state.verified.get(&resource.id) == Some(&resource.stamp) {
                state.verified.remove(&resource.id);
                self.publish(&state);
            }
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
                Arc::new(move |files: bool| owner.current(&captured, files)),
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
            Arc::new(move |files: bool| owner.current(&captured, files)),
            operation,
        )
    }

    pub(crate) fn cancel(&self, id: &str) -> bool {
        self.lock().transfers.get(id).is_some_and(|transfer| {
            transfer.cancel();
            true
        })
    }

    pub(crate) async fn shutdown(&self) {
        self.stop.cancel();
        self.runtime.shutdown().await;
    }
}

/// Applies an inspection unless installation, removal or another inspection changed it.
fn commit(
    state: &mut Configuration,
    id: &str,
    definition: &Arc<Definition>,
    cached: Option<&Stamp>,
    stamp: Option<Stamp>,
) -> bool {
    if state.removing.contains(id)
        || state.definitions.get(id) != Some(definition)
        || state.verified.get(id) != cached
    {
        return false;
    }
    match stamp {
        Some(stamp) => state.verified.insert(id.into(), stamp),
        None => state.verified.remove(id),
    };
    true
}

fn update(state: &Configuration) -> Update {
    Update {
        models: infos(state),
        embedding: state.settings.choice(Kind::Embedding),
        ocr: state.settings.choice(Kind::Ocr),
    }
}

fn infos(state: &Configuration) -> Vec<Model> {
    state
        .definitions
        .iter()
        .map(|(id, definition)| Model {
            id: id.clone(),
            definition: definition.as_ref().clone(),
            state: if state.transfers.contains_key(id) {
                State::Downloading
            } else if !state.removing.contains(id) && state.verified.contains_key(id) {
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
fn missing() -> Fault {
    Fault::new(ErrorCode::NotConfigured, "local model is not installed")
}
fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "local model configuration or assets changed",
    )
}
