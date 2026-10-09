//! Explicit model acquisition and one bounded, lazy CPU inference owner per Node.
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    knowledge::{Model, ModelInfo, ModelState, Registration, Settings},
    models::{Definition, Format},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, mpsc, oneshot, watch};

pub(crate) mod catalog;
mod files;
#[cfg(test)]
mod tests;
mod worker;

const IDLE: Duration = Duration::from_secs(5 * 60);
const BATCH: usize = 4;

#[derive(Clone, Copy)]
pub(super) enum Lane {
    Query,
    Index,
}

pub(crate) struct Models {
    root: PathBuf,
    directory: Arc<cap_std::fs::Dir>,
    state: Mutex<State>,
    query: mpsc::Sender<Work>,
    index: mpsc::Sender<Work>,
    control: mpsc::Sender<worker::Control>,
    updates: watch::Sender<Vec<ModelInfo>>,
    finished: Notify,
    stop: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    #[cfg(any(test, feature = "test-support"))]
    indexed: std::sync::atomic::AtomicU64,
}

struct State {
    definitions: BTreeMap<Model, Arc<Definition>>,
    settings: Settings,
    generation: u64,
    epochs: BTreeMap<Model, u64>,
    receipts: BTreeMap<Model, files::Stamp>,
    active: BTreeMap<Model, Transfer>,
    removing: BTreeSet<Model>,
}

struct Transfer {
    cancel: CancellationToken,
    percent: u8,
    speed: u64,
    published: Instant,
}

struct Work {
    model: Model,
    format: Format,
    revision: u64,
    generation: u64,
    epoch: u64,
    stamp: files::Stamp,
    input: Input,
    cancel: CancellationToken,
    reply: oneshot::Sender<Result<Output, Fault>>,
}

#[derive(Clone)]
enum Input {
    Encode(Vec<String>),
    Offsets(String),
}
enum Output {
    Vectors(Vec<Vec<f32>>),
    Offsets(Vec<(usize, usize)>),
}

impl Models {
    pub async fn open(
        profile: &Path,
        settings: Settings,
        records: Vec<Registration>,
    ) -> Result<Arc<Self>, Fault> {
        let definitions = catalog::definitions(&records)?;
        let profile = profile.to_owned();
        let (root, directory, definitions, receipts) = tokio::task::spawn_blocking(move || {
            let (root, directory) = files::open(&profile)?;
            let receipts = definitions
                .iter()
                .filter_map(|(model, definition)| {
                    files::verified(&root, *model, definition).map(|stamp| (*model, stamp))
                })
                .collect();
            Ok::<_, Fault>((root, directory, definitions, receipts))
        })
        .await
        .map_err(|_| unavailable())??;
        Self::start(root, directory, settings, definitions, receipts, IDLE)
    }

    fn start(
        root: PathBuf,
        directory: cap_std::fs::Dir,
        settings: Settings,
        definitions: BTreeMap<Model, Arc<Definition>>,
        receipts: BTreeMap<Model, files::Stamp>,
        idle: Duration,
    ) -> Result<Arc<Self>, Fault> {
        Self::validate(&settings)?;
        selected(&settings, &definitions)?;
        let (query, queries) = mpsc::channel(8);
        let (index, indexes) = mpsc::channel(4);
        let (control, controls) = mpsc::channel(4);
        let state = State {
            definitions,
            settings,
            generation: 0,
            epochs: BTreeMap::new(),
            receipts,
            active: BTreeMap::new(),
            removing: BTreeSet::new(),
        };
        let (updates, _) = watch::channel(infos(&state));
        let owner = Arc::new(Self {
            root,
            directory: Arc::new(directory),
            state: Mutex::new(state),
            query,
            index,
            control,
            updates,
            finished: Notify::new(),
            stop: CancellationToken::new(),
            task: Mutex::new(None),
            #[cfg(any(test, feature = "test-support"))]
            indexed: std::sync::atomic::AtomicU64::new(0),
        });
        let task = tokio::spawn(worker::run(
            Arc::downgrade(&owner),
            queries,
            indexes,
            controls,
            owner.stop.clone(),
            idle,
        ));
        *owner.task.lock().expect("embedding worker lock is healthy") = Some(task);
        Ok(owner)
    }

    pub fn configure(&self, settings: Settings, records: &[Registration]) -> Result<(), Fault> {
        Self::validate(&settings)?;
        let definitions = catalog::definitions(records)?;
        selected(&settings, &definitions)?;
        let mut state = self.state.lock().expect("embedding state lock is healthy");
        for (model, previous) in &state.definitions {
            if definitions.get(model).is_none_or(|next| next != previous) {
                return Err(Fault::invalid("registered model definition is immutable"));
            }
        }
        if state.definitions != definitions {
            state.definitions = definitions;
            self.updates.send_replace(infos(&state));
        }
        if state.settings != settings {
            state.settings = settings;
            state.generation = state.generation.wrapping_add(1);
            for transfer in state.active.values() {
                transfer.cancel.cancel();
            }
            let _ = self.control.try_send(worker::Control::Unload {
                model: None,
                reply: None,
            });
        }
        Ok(())
    }

    pub fn validate(settings: &Settings) -> Result<(), Fault> {
        source(settings).map(|_| ())
    }

    fn definition(&self, model: Model) -> Result<Arc<Definition>, Fault> {
        self.state
            .lock()
            .expect("embedding state lock is healthy")
            .definitions
            .get(&model)
            .cloned()
            .ok_or_else(|| Fault::invalid("embedding model is not registered"))
    }

    pub fn subscribe(&self) -> watch::Receiver<Vec<ModelInfo>> {
        self.updates.subscribe()
    }

    pub async fn infos(&self) -> Result<Vec<ModelInfo>, Fault> {
        self.refresh().await?;
        Ok(infos(
            &self.state.lock().expect("embedding state lock is healthy"),
        ))
    }

    async fn refresh(&self) -> Result<(), Fault> {
        let root = self.root.clone();
        let receipts = {
            let state = self.state.lock().expect("embedding state lock is healthy");
            state
                .receipts
                .iter()
                .filter_map(|(model, stamp)| {
                    state
                        .definitions
                        .get(model)
                        .map(|definition| (*model, stamp.clone(), definition.clone()))
                })
                .collect::<Vec<_>>()
        };
        let observed = tokio::task::spawn_blocking(move || {
            receipts
                .into_iter()
                .map(|(model, receipt, definition)| {
                    (model, receipt, files::stamp(&root, model, &definition))
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|_| unavailable())?;
        let mut state = self.state.lock().expect("embedding state lock is healthy");
        let mut changed = false;
        for (model, receipt, observed) in observed {
            if state.receipts.get(&model) == Some(&receipt) && observed.as_ref() != Some(&receipt) {
                state.receipts.remove(&model);
                *state.epochs.entry(model).or_default() += 1;
                let _ = self.control.try_send(worker::Control::Unload {
                    model: Some(model),
                    reply: None,
                });
                changed = true;
            }
        }
        if changed {
            self.updates.send_replace(infos(&state));
        }
        Ok(())
    }

    pub async fn install(
        self: &Arc<Self>,
        model: Model,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        if cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        self.refresh().await?;
        let (mirror, definition, generation, transfer) = {
            let mut state = self.state.lock().expect("embedding state lock is healthy");
            if !state.active.is_empty() {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "an embedding model is already downloading",
                ));
            }
            if state.removing.contains(&model) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "embedding model is being removed",
                ));
            }
            if state.receipts.contains_key(&model) {
                return Ok(());
            }
            let mirror = source(&state.settings)?;
            let definition = state
                .definitions
                .get(&model)
                .cloned()
                .ok_or_else(|| Fault::invalid("embedding model is not registered"))?;
            let cancel = self.stop.child_token();
            state.active.insert(
                model,
                Transfer {
                    cancel: cancel.clone(),
                    percent: 0,
                    speed: 0,
                    published: Instant::now(),
                },
            );
            self.updates.send_replace(infos(&state));
            (mirror, definition, state.generation, cancel)
        };
        let _guard = Installation {
            owner: self.clone(),
            model,
        };
        let specifications = definition
            .assets
            .iter()
            .map(|asset| {
                Ok(sailry_model_assets::Download {
                    file: sailry_model_assets::File {
                        name: &asset.name,
                        size: asset.bytes,
                        sha256: &asset.sha256,
                    },
                    url: sailry_model_assets::url(asset, Some(&mirror)).map_err(asset_failure)?,
                })
            })
            .collect::<Result<Vec<_>, Fault>>()?;
        let directory = files::directory(&self.root, model);
        let operation = async {
            sailry_model_assets::prepare(&directory, &specifications, &transfer, |progress| {
                self.progress(model, progress)
            })
            .await
            .map_err(asset_failure)?;
            let root = self.root.clone();
            let definition = definition.clone();
            let native = self.directory.clone();
            let stopping = transfer.clone();
            let receipt = tokio::task::spawn_blocking(move || {
                sailry_embedding::background();
                let directory = files::directory(&root, model);
                let mut engine =
                    sailry_embedding::Engine::load_format(definition.format, &directory)
                        .map_err(worker::failure)?;
                let vectors = engine
                    .encode(&["Model compatibility check"])
                    .map_err(worker::failure)?;
                if vectors.len() != 1 || stopping.is_cancelled() {
                    return Err(cancelled());
                }
                files::mark(&native, model, &definition)?;
                files::verified(&root, model, &definition).ok_or_else(unavailable)
            })
            .await
            .map_err(|_| unavailable())??;
            let mut state = self.state.lock().expect("embedding state lock is healthy");
            if state.generation != generation || transfer.is_cancelled() {
                return Err(cancelled());
            }
            state.receipts.insert(model, receipt);
            *state.epochs.entry(model).or_default() += 1;
            Ok(())
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => { transfer.cancel(); Err(cancelled()) },
            _ = transfer.cancelled() => Err(cancelled()),
            result = operation => result,
        }
    }

    fn progress(&self, model: Model, progress: sailry_model_assets::Progress) {
        let mut state = self.state.lock().expect("embedding state lock is healthy");
        let Some(transfer) = state.active.get_mut(&model) else {
            return;
        };
        transfer.percent = progress.percent.clamp(0.0, 99.0) as u8;
        transfer.speed = progress.bytes_per_second.max(0.0) as u64;
        if transfer.published.elapsed() >= Duration::from_millis(250) {
            transfer.published = Instant::now();
            self.updates.send_replace(infos(&state));
        }
    }

    pub fn cancel(&self, model: Model) -> bool {
        let state = self.state.lock().expect("embedding state lock is healthy");
        if let Some(transfer) = state.active.get(&model) {
            transfer.cancel.cancel();
            true
        } else {
            false
        }
    }

    pub async fn remove(self: &Arc<Self>, model: Model) -> Result<(), Fault> {
        self.definition(model)?;
        {
            let mut state = self.state.lock().expect("embedding state lock is healthy");
            if !state.removing.insert(model) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "embedding model is being removed",
                ));
            }
            state.receipts.remove(&model);
            *state.epochs.entry(model).or_default() += 1;
            if let Some(transfer) = state.active.get(&model) {
                transfer.cancel.cancel();
            }
            self.updates.send_replace(infos(&state));
        }
        let guard = Removal {
            owner: self.clone(),
            model,
        };
        loop {
            let notified = self.finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self
                .state
                .lock()
                .expect("embedding state lock is healthy")
                .active
                .contains_key(&model)
            {
                break;
            }
            tokio::select! { biased; _ = self.stop.cancelled() => return Err(cancelled()), _ = notified => () }
        }
        let (reply, result) = oneshot::channel();
        self.control
            .send(worker::Control::Unload {
                model: Some(model),
                reply: Some(reply),
            })
            .await
            .map_err(|_| unavailable())?;
        result.await.map_err(|_| unavailable())?;
        let directory = self.directory.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            files::remove(&directory, model)
        })
        .await
        .map_err(|_| unavailable())?
    }

    pub(super) async fn encode(
        &self,
        model: Model,
        revision: u64,
        texts: Vec<String>,
        lane: Lane,
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, Fault> {
        if texts.len() > BATCH {
            return Err(Fault::invalid("embedding batch exceeds four inputs"));
        }
        match self
            .request(model, revision, Input::Encode(texts), lane, cancel)
            .await?
        {
            Output::Vectors(vectors) => {
                #[cfg(any(test, feature = "test-support"))]
                if matches!(lane, Lane::Index) {
                    self.indexed
                        .fetch_add(vectors.len() as u64, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(vectors)
            }
            Output::Offsets(_) => Err(Fault::new(
                ErrorCode::Internal,
                "unexpected embedding reply",
            )),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn indexed_inputs(&self) -> u64 {
        self.indexed.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn offsets(
        &self,
        model: Model,
        revision: u64,
        text: String,
        cancel: &CancellationToken,
    ) -> Result<Vec<(usize, usize)>, Fault> {
        match self
            .request(model, revision, Input::Offsets(text), Lane::Index, cancel)
            .await?
        {
            Output::Offsets(offsets) => Ok(offsets),
            Output::Vectors(_) => Err(Fault::new(
                ErrorCode::Internal,
                "unexpected embedding reply",
            )),
        }
    }

    async fn request(
        &self,
        model: Model,
        revision: u64,
        input: Input,
        lane: Lane,
        cancel: &CancellationToken,
    ) -> Result<Output, Fault> {
        if cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        self.refresh().await?;
        let (reply, result) = oneshot::channel();
        let work = {
            let state = self.state.lock().expect("embedding state lock is healthy");
            if state.settings.revision != revision {
                return Err(conflict());
            }
            let stamp = state.receipts.get(&model).cloned().ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "embedding model is not installed")
            })?;
            Work {
                model,
                format: state
                    .definitions
                    .get(&model)
                    .ok_or_else(|| Fault::invalid("embedding model is not registered"))?
                    .format,
                revision,
                generation: state.generation,
                epoch: *state.epochs.get(&model).unwrap_or(&0),
                stamp,
                input,
                cancel: cancel.clone(),
                reply,
            }
        };
        let generation = work.generation;
        let epoch = work.epoch;
        let queue = match lane {
            Lane::Query => &self.query,
            Lane::Index => &self.index,
        };
        queue.try_send(work).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                Fault::new(ErrorCode::Busy, "embedding queue is full")
            }
            mpsc::error::TrySendError::Closed(_) => unavailable(),
        })?;
        let vectors = tokio::select! { biased; _ = cancel.cancelled() => return Err(cancelled()), _ = self.stop.cancelled() => return Err(cancelled()), result = result => result.map_err(|_| unavailable())?? };
        let state = self.state.lock().expect("embedding state lock is healthy");
        if state.generation != generation
            || state.settings.revision != revision
            || *state.epochs.get(&model).unwrap_or(&0) != epoch
            || !state.receipts.contains_key(&model)
        {
            return Err(conflict());
        }
        Ok(vectors)
    }

    fn current(&self, work: &Work) -> Result<(), Fault> {
        if work.cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        let state = self.state.lock().expect("embedding state lock is healthy");
        if state.settings.revision != work.revision
            || state.generation != work.generation
            || *state.epochs.get(&work.model).unwrap_or(&0) != work.epoch
            || state.receipts.get(&work.model) != Some(&work.stamp)
        {
            return Err(conflict());
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        self.stop.cancel();
        let task = self
            .task
            .lock()
            .expect("embedding worker lock is healthy")
            .take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

impl Drop for Models {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Installation {
    owner: Arc<Models>,
    model: Model,
}
impl Drop for Installation {
    fn drop(&mut self) {
        let mut state = self
            .owner
            .state
            .lock()
            .expect("embedding state lock is healthy");
        if let Some(transfer) = state.active.remove(&self.model) {
            transfer.cancel.cancel();
        }
        self.owner.updates.send_replace(infos(&state));
        self.owner.finished.notify_waiters();
    }
}

struct Removal {
    owner: Arc<Models>,
    model: Model,
}
impl Drop for Removal {
    fn drop(&mut self) {
        self.owner
            .state
            .lock()
            .expect("embedding state lock is healthy")
            .removing
            .remove(&self.model);
    }
}

fn infos(state: &State) -> Vec<ModelInfo> {
    state
        .definitions
        .iter()
        .map(|(model, definition)| ModelInfo {
            model: *model,
            repository: definition.source.repository.clone(),
            revision: definition.source.revision.clone(),
            dimension: definition.format.dimension().unwrap_or_default(),
            size_bytes: definition.assets.iter().map(|asset| asset.bytes).sum(),
            license: definition.license.clone(),
            definition: definition.as_ref().clone(),
            state: if let Some(transfer) = state.active.get(model) {
                ModelState::Downloading {
                    percent: transfer.percent,
                    bytes_per_second: transfer.speed,
                }
            } else if state.receipts.contains_key(model) {
                ModelState::Installed
            } else {
                ModelState::Missing
            },
        })
        .collect()
}

#[cfg(test)]
fn descriptor(model: Model) -> &'static sailry_embedding::catalog::Model {
    use sailry_embedding::ModelId;
    match model {
        Model::GraniteMulti => ModelId::GraniteMulti,
        Model::BgeSmallZh => ModelId::BgeSmallZh,
        Model::PotionMulti => ModelId::PotionMulti,
        Model::Registered { .. } => panic!("test descriptor requires a recommended model"),
    }
    .descriptor()
}

fn source(settings: &Settings) -> Result<String, Fault> {
    let base = settings
        .mirror
        .as_deref()
        .unwrap_or("https://huggingface.co");
    let parsed =
        url::Url::parse(base).map_err(|_| Fault::invalid("invalid embedding mirror URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Fault::invalid(
            "embedding mirror requires HTTPS without credentials",
        ));
    }
    Ok(base.trim_end_matches('/').to_owned())
}

fn selected(
    settings: &Settings,
    definitions: &BTreeMap<Model, Arc<Definition>>,
) -> Result<(), Fault> {
    if settings
        .model
        .is_some_and(|model| !definitions.contains_key(&model))
    {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "embedding model is not registered",
        ));
    }
    Ok(())
}

fn asset_failure(error: sailry_model_assets::Error) -> Fault {
    match error {
        sailry_model_assets::Error::Cancelled => cancelled(),
        sailry_model_assets::Error::InvalidAsset => {
            Fault::invalid("embedding model files failed size or SHA-256 validation")
        }
        sailry_model_assets::Error::Storage => Fault::new(
            ErrorCode::Unavailable,
            "embedding model files could not be saved",
        ),
        sailry_model_assets::Error::Download => Fault::new(
            ErrorCode::Unavailable,
            "embedding model could not be downloaded",
        ),
    }
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "embedding model is unavailable")
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "embedding operation was cancelled")
}
fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "embedding configuration changed",
    )
}
