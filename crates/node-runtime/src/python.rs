//! Bundled Python and shared, reproducible plugin environments belong to the Node.
mod environment;
mod execution;
mod integrity;
mod process;
mod requirements;
mod runtime;
mod validation;

#[cfg(test)]
mod tests;

#[cfg(test)]
use std::path::Path;

pub(crate) use execution::Execution;
pub(crate) use execution::remove_session;
pub(crate) use requirements::Requirements;
pub(crate) use runtime::Runtime;

use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime},
};
use tokio::sync::{OnceCell, Semaphore, watch};

pub(crate) const VERSION: &str = "3.12.13";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Creating,
    Installing,
    Validating,
    Ready,
    Failed,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    pub package: String,
    pub stage: Stage,
}

#[derive(Clone)]
pub(crate) struct Ready {
    pub python: PathBuf,
    pub environment: PathBuf,
    pub runtime: PathBuf,
    // A captured skill/MCP launch keeps its environment out of idle cleanup.
    _lease: Option<Arc<()>>,
    requirements: Option<Arc<Requirements>>,
}

#[derive(Clone)]
pub(crate) struct Access {
    pub ready: Ready,
    pub directory: PathBuf,
}

pub(crate) struct Manager {
    runtime: Runtime,
    root: PathBuf,
    environment: Arc<Environment>,
    index: Mutex<Option<String>>,
    environments: Mutex<BTreeMap<String, Arc<Prepared>>>,
    checked: OnceCell<()>,
    #[cfg(test)]
    progress: watch::Sender<Option<Progress>>,
    preparations: Arc<watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    active: Mutex<BTreeMap<String, Active>>,
    leases: Arc<Mutex<BTreeMap<String, Weak<()>>>>,
    references: Mutex<Option<BTreeSet<String>>>,
    collected: Mutex<Option<SystemTime>>,
}

struct Prepared {
    installer: Semaphore,
    validation: Mutex<validation::Validation>,
}

#[derive(Default)]
struct Active {
    stage: Option<sailry_protocol::python::Stage>,
    packages: BTreeMap<String, usize>,
}

impl Manager {
    #[cfg(test)]
    pub(crate) fn new(
        profile: PathBuf,
        runtime: Runtime,
        environment: Arc<Environment>,
    ) -> Result<Self, Fault> {
        Self::with_progress(
            profile,
            runtime,
            environment,
            Arc::new(watch::channel(Vec::new()).0),
        )
    }

    pub(crate) fn with_progress(
        profile: PathBuf,
        runtime: Runtime,
        environment: Arc<Environment>,
        preparations: Arc<watch::Sender<Vec<sailry_protocol::python::Preparation>>>,
    ) -> Result<Self, Fault> {
        let profile = profile.canonicalize().map_err(storage)?;
        let root = profile.join("python");
        for path in [
            &root,
            &root.join("environments"),
            &root.join("staging"),
            &root.join("cache"),
        ] {
            crate::profile::private_directory(path).map_err(storage)?;
        }
        Ok(Self {
            runtime,
            root,
            environment,
            index: Mutex::new(None),
            environments: Mutex::default(),
            checked: OnceCell::new(),
            #[cfg(test)]
            progress: watch::channel(None).0,
            preparations,
            active: Mutex::default(),
            leases: Arc::default(),
            references: Mutex::new(None),
            collected: Mutex::new(None),
        })
    }

    pub(crate) fn configure(&self, index: Option<String>) -> Result<(), Fault> {
        validate_index(index.as_deref())?;
        *self.index.lock().expect("Python index lock is valid") = index;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn python(&self) -> &Path {
        &self.runtime.python
    }

    #[cfg(test)]
    pub(crate) fn subscribe(&self) -> watch::Receiver<Option<Progress>> {
        self.progress.subscribe()
    }

    /// Installed-package references are supplied by their existing Store owner.
    pub(crate) fn retain(&self, requirements: &[Requirements]) {
        *self
            .references
            .lock()
            .expect("Python references lock is valid") = Some(
            requirements
                .iter()
                .map(|requirements| requirements.key(&self.runtime.identity))
                .collect(),
        );
    }

    pub(crate) async fn ensure(
        &self,
        requirements: Option<&Requirements>,
        cancel: &CancellationToken,
    ) -> Result<Ready, Fault> {
        check(cancel)?;
        let runtime = self.runtime.clone();
        let stop = cancel.clone();
        tokio::task::spawn_blocking(move || runtime.verify(&stop))
            .await
            .map_err(|_| unavailable())??;
        self.checked
            .get_or_try_init(|| self.runtime.check(&self.environment, cancel))
            .await?;
        check(cancel)?;
        let Some(requirements) = requirements else {
            return Ok(Ready {
                python: self.runtime.python.clone(),
                environment: self.runtime.root.clone(),
                runtime: self.runtime.root.clone(),
                _lease: None,
                requirements: None,
            });
        };
        let key = requirements.key(&self.runtime.identity);
        let _requester = self.request(requirements);
        let path = self.root.join("environments").join(&key);
        let lease = {
            let mut leases = self.leases.lock().expect("Python lease lock is valid");
            let lease = leases.get(&key).and_then(Weak::upgrade).unwrap_or_default();
            leases.insert(key.clone(), Arc::downgrade(&lease));
            lease
        };
        let prepared = {
            let mut environments = self
                .environments
                .lock()
                .expect("Python environments lock is valid");
            environments
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(Prepared {
                        installer: Semaphore::new(1),
                        validation: Mutex::new(validation::Validation::new(path.clone())),
                    })
                })
                .clone()
        };
        if !self.valid(&prepared, &key, cancel).await? {
            let _permit = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                permit = prepared.installer.acquire() => permit.map_err(|_| unavailable())?,
            };
            // Another caller may have prepared this same lock while we waited.
            if !self.valid(&prepared, &key, cancel).await? {
                let _progress = Preparing {
                    manager: self,
                    requirements,
                };
                let result = self.prepare(requirements, &key, &path, cancel).await;
                if result.is_err() {
                    self.report(requirements, Stage::Failed);
                }
                result?;
                if !self.valid(&prepared, &key, cancel).await? {
                    self.report(requirements, Stage::Failed);
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "prepared Python environment failed validation",
                    ));
                }
            }
        }
        check(cancel)?;
        std::fs::write(path.join(".sailry-used"), b"").map_err(storage)?;
        if let Err(error) = self.collect(&key).await {
            eprintln!("Python environment cleanup failed: {error}");
        }
        self.report(requirements, Stage::Ready);
        Ok(Ready {
            python: runtime::python(&path),
            environment: path,
            runtime: self.runtime.root.clone(),
            _lease: Some(lease),
            requirements: Some(Arc::new(requirements.clone())),
        })
    }

    pub(crate) async fn refresh(
        &self,
        ready: &Ready,
        cancel: &CancellationToken,
    ) -> Result<Ready, Fault> {
        self.ensure(ready.requirements.as_deref(), cancel).await
    }

    async fn valid(
        &self,
        prepared: &Arc<Prepared>,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<bool, Fault> {
        let runtime = self.runtime.root.clone();
        let key = key.to_owned();
        let prepared = prepared.clone();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let mut validation = prepared
                .validation
                .lock()
                .expect("Python integrity lock is valid");
            validation.environment(&runtime, &key, &cancel)
        })
        .await
        .map_err(|_| unavailable())?
    }

    fn report(&self, requirements: &Requirements, stage: Stage) {
        #[cfg(test)]
        self.progress.send_replace(Some(Progress {
            package: requirements.package.name.clone(),
            stage: stage.clone(),
        }));
        let key = requirements.key(&self.runtime.identity);
        let mut active = self.active.lock().expect("Python progress lock is valid");
        use sailry_protocol::python;
        let stage = match stage {
            Stage::Creating => Some(python::Stage::Creating),
            Stage::Installing => Some(python::Stage::Installing),
            Stage::Validating => Some(python::Stage::Validating),
            Stage::Ready | Stage::Failed => None,
        };
        let entry = active.entry(key.clone()).or_default();
        entry.stage = stage;
        if stage.is_none() && entry.packages.is_empty() {
            active.remove(&key);
        }
        self.publish(&active);
    }

    fn finish(&self, requirements: &Requirements) {
        let key = requirements.key(&self.runtime.identity);
        let mut active = self.active.lock().expect("Python progress lock is valid");
        if let Some(entry) = active.get_mut(&key) {
            entry.stage = None;
            if entry.packages.is_empty() {
                active.remove(&key);
            }
            self.publish(&active);
        }
    }

    fn request<'a>(&'a self, requirements: &'a Requirements) -> Requester<'a> {
        let key = requirements.key(&self.runtime.identity);
        let mut active = self.active.lock().expect("Python progress lock is valid");
        *active
            .entry(key)
            .or_default()
            .packages
            .entry(requirements.package.name.clone())
            .or_default() += 1;
        self.publish(&active);
        Requester {
            manager: self,
            requirements,
        }
    }

    fn release(&self, requirements: &Requirements) {
        let key = requirements.key(&self.runtime.identity);
        let mut active = self.active.lock().expect("Python progress lock is valid");
        if let Some(entry) = active.get_mut(&key) {
            if let Some(count) = entry.packages.get_mut(&requirements.package.name) {
                *count -= 1;
                if *count == 0 {
                    entry.packages.remove(&requirements.package.name);
                }
            }
            if entry.stage.is_none() && entry.packages.is_empty() {
                active.remove(&key);
            }
            self.publish(&active);
        }
    }

    fn publish(&self, active: &BTreeMap<String, Active>) {
        self.preparations.send_if_modified(|snapshot| {
            let current = active
                .values()
                .flat_map(|entry| {
                    entry.packages.keys().filter_map(|package| {
                        entry
                            .stage
                            .map(|stage| sailry_protocol::python::Preparation {
                                package: package.clone(),
                                stage,
                            })
                    })
                })
                .collect::<Vec<_>>();
            if *snapshot == current {
                return false;
            }
            *snapshot = current;
            true
        });
    }

    pub(crate) fn collection_due(&self) -> bool {
        self.collected
            .lock()
            .expect("Python cleanup lock is valid")
            .is_none_or(|last| {
                SystemTime::now()
                    .duration_since(last)
                    .is_ok_and(|age| age >= Duration::from_secs(24 * 60 * 60))
            })
    }

    async fn collect(&self, current: &str) -> Result<(), Fault> {
        const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
        const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
        let Some(references) = self
            .references
            .lock()
            .expect("Python references lock is valid")
            .clone()
        else {
            return Ok(());
        };
        let now = SystemTime::now();
        {
            let mut collected = self.collected.lock().expect("Python cleanup lock is valid");
            if collected
                .is_some_and(|last| now.duration_since(last).is_ok_and(|age| age < INTERVAL))
            {
                return Ok(());
            }
            *collected = Some(now);
        }
        let leases = self.leases.clone();
        let root = self.root.clone();
        let current = current.to_owned();
        let removed = tokio::task::spawn_blocking(move || {
            let mut removed = Vec::new();
            for entry in std::fs::read_dir(root.join("environments")).map_err(storage)? {
                let entry = entry.map_err(storage)?;
                let Some(key) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if key == current || references.contains(&key) || !integrity::key(&key) {
                    continue;
                }
                let path = entry.path();
                let metadata = path.symlink_metadata().map_err(storage)?;
                if !metadata.is_dir() || metadata.is_symlink() {
                    continue;
                }
                let used = path
                    .join(".sailry-used")
                    .symlink_metadata()
                    .ok()
                    .filter(|metadata| metadata.is_file() && !metadata.is_symlink())
                    .and_then(|metadata| metadata.modified().ok());
                if used
                    .is_some_and(|used| now.duration_since(used).is_ok_and(|age| age >= RETENTION))
                {
                    // Taking the lease lock through deletion prevents a newly
                    // admitted use racing this environment's idle cleanup.
                    let mut leases = leases.lock().expect("Python lease lock is valid");
                    if leases.get(&key).and_then(Weak::upgrade).is_some() {
                        continue;
                    }
                    std::fs::remove_dir_all(path).map_err(storage)?;
                    leases.remove(&key);
                    removed.push(key);
                }
            }
            Ok::<_, Fault>(removed)
        })
        .await
        .map_err(|_| unavailable())??;
        let mut environments = self
            .environments
            .lock()
            .expect("Python environments lock is valid");
        for key in removed {
            // A caller may have begun recreating this entry after collection.
            // Its retained state remains authoritative through preparation.
            if environments
                .get(&key)
                .is_some_and(|prepared| Arc::strong_count(prepared) == 1)
            {
                environments.remove(&key);
            }
        }
        Ok(())
    }
}

struct Preparing<'a> {
    manager: &'a Manager,
    requirements: &'a Requirements,
}

impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        self.manager.finish(self.requirements);
    }
}

struct Requester<'a> {
    manager: &'a Manager,
    requirements: &'a Requirements,
}

impl Drop for Requester<'_> {
    fn drop(&mut self) {
        self.manager.release(self.requirements);
    }
}

fn check(cancel: &CancellationToken) -> Result<(), Fault> {
    if cancel.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

fn cancelled() -> Fault {
    Fault::new(
        ErrorCode::Cancelled,
        "Python environment preparation was cancelled",
    )
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "Python environment preparation is unavailable",
    )
}

fn storage(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("Python environment storage failed: {error}"),
    )
}

pub(crate) fn validate_index(index: Option<&str>) -> Result<(), Fault> {
    if let Some(index) = index {
        let url = reqwest::Url::parse(index)
            .map_err(|_| Fault::invalid("invalid Python package index URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || index.len() > 4096
        {
            return Err(Fault::invalid(
                "Python package index must be an HTTP URL without credentials",
            ));
        }
    }
    Ok(())
}
