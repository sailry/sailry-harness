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
    updates: watch::Sender<Update>,
    runtime: Runtime,
    stop: CancellationToken,
}

struct Configuration {
    settings: Settings,
    definitions: BTreeMap<String, Arc<Definition>>,
    verified: BTreeMap<String, Stamp>,
    epochs: BTreeMap<String, u64>,
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
            verified: BTreeMap::new(),
            epochs: BTreeMap::new(),
            removing: BTreeSet::new(),
            transfers: BTreeMap::new(),
            scopes: Kind::NODE
                .into_iter()
                .map(|kind| (kind, stop.child_token()))
                .collect(),
        };
        let (updates, _) = watch::channel(update(&state, infos(&state, &[])));
        Arc::new(Self {
            profile,
            root: OnceLock::new(),
            state: Mutex::new(state),
            updates,
            runtime: Runtime::new(stop.child_token()),
            stop,
        })
    }

    pub(crate) fn prepare(
        &self,
        mut settings: Settings,
        records: &[Registration],
    ) -> Result<Prepared, Fault> {
        settings.fill_defaults();
        settings.validate()?;
        let definitions = catalog::definitions(records)?;
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

    pub(crate) fn apply(&self, prepared: Prepared) {
        let Prepared {
            settings,
            definitions,
        } = prepared;
        let mut state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
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
        state.verified.retain(|id, _| definitions.contains_key(id));
        state.settings = settings;
        state.definitions = definitions;
        self.publish(&state);
    }

    #[cfg(test)]
    pub(crate) fn configure(
        &self,
        settings: Settings,
        records: &[Registration],
    ) -> Result<(), Fault> {
        self.apply(self.prepare(settings, records)?);
        Ok(())
    }

    pub(crate) fn settings(&self) -> Settings {
        self.state
            .lock()
            .expect("local model configuration lock is healthy")
            .settings
            .clone()
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
        let root = self.profile.as_ref().map(|_| self.root()).transpose()?;
        let mut state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        let mut models = infos(&state, &[]);
        if let Some(root) = root {
            for model in &mut models {
                if model.state == State::Missing && !state.removing.contains(&model.id) {
                    let directory = files::directory(&root, &model.id, &model.definition)?;
                    if inspect(&mut state, &directory, &model.id, &model.definition).is_some() {
                        model.state = State::Installed;
                    }
                }
            }
        }
        self.publish_models(&state, models.clone());
        Ok(models
            .into_iter()
            .filter(|model| kind.is_none_or(|kind| model.definition.format.kind() == kind))
            .collect())
    }

    fn publish(&self, state: &Configuration) {
        let models = infos(state, &self.updates.borrow().models);
        self.publish_models(state, models);
    }

    fn publish_models(&self, state: &Configuration, models: Vec<Model>) {
        let latest = update(state, models);
        self.updates.send_if_modified(|previous| {
            if *previous == latest {
                return false;
            }
            *previous = latest;
            true
        });
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
        id: &str,
        choice: Option<&Choice>,
    ) -> Result<Resource, Fault> {
        let root = self.root()?;
        let mut state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        if choice.is_some_and(|choice| state.settings.choice(kind) != *choice) {
            return Err(conflict());
        }
        let definition = state
            .definitions
            .get(id)
            .filter(|definition| definition.format.kind() == kind)
            .cloned()
            .ok_or_else(|| Fault::invalid("local model kind does not match the request"))?;
        let directory = files::directory(&root, id, &definition)?;
        let stamp = (!state.removing.contains(id))
            .then(|| inspect(&mut state, &directory, id, &definition))
            .flatten()
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "local model is not installed"))?;
        Ok(Resource {
            id: id.into(),
            directory,
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
            || state.definitions.get(&resource.id) != Some(&resource.definition)
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

    pub(crate) fn cancel(&self, id: &str) -> bool {
        let state = self
            .state
            .lock()
            .expect("local model configuration lock is healthy");
        state.transfers.get(id).is_some_and(|transfer| {
            transfer.cancel();
            true
        })
    }

    pub(crate) async fn shutdown(&self) {
        self.stop.cancel();
        self.runtime.shutdown().await;
    }
}

fn inspect(
    state: &mut Configuration,
    directory: &std::path::Path,
    id: &str,
    definition: &Definition,
) -> Option<Stamp> {
    let stamp = files::stamp(directory, id, definition, state.verified.get(id));
    match &stamp {
        Some(stamp) => {
            state.verified.insert(id.into(), stamp.clone());
        }
        None => {
            state.verified.remove(id);
        }
    }
    stamp
}

fn update(state: &Configuration, models: Vec<Model>) -> Update {
    Update {
        models,
        embedding: state.settings.choice(Kind::Embedding),
    }
}

fn infos(state: &Configuration, observed: &[Model]) -> Vec<Model> {
    state
        .definitions
        .iter()
        .map(|(id, definition)| Model {
            id: id.clone(),
            definition: definition.as_ref().clone(),
            state: if state.transfers.contains_key(id) {
                State::Downloading
            } else if !state.removing.contains(id)
                && observed
                    .iter()
                    .any(|model| model.id == *id && model.state == State::Installed)
            {
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
