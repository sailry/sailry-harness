//! Bundled Python and shared, reproducible plugin environments belong to the Node.
mod environment;
mod execution;
mod integrity;
mod process;
mod requirements;
mod runtime;

#[cfg(test)]
mod tests;

pub(crate) use execution::Execution;
pub(crate) use execution::remove_session;
pub(crate) use requirements::Requirements;
pub(crate) use runtime::Runtime;

use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
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
    installer: Semaphore,
    checked: OnceCell<()>,
    progress: watch::Sender<Option<Progress>>,
    integrity: Arc<Mutex<integrity::Cache>>,
    leases: Mutex<BTreeMap<String, Weak<()>>>,
    references: Mutex<Option<BTreeSet<String>>>,
    collected: Mutex<Option<SystemTime>>,
}

impl Manager {
    pub(crate) fn new(
        profile: PathBuf,
        runtime: Runtime,
        environment: Arc<Environment>,
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
        let (progress, _) = watch::channel(None);
        Ok(Self {
            runtime,
            root,
            environment,
            index: Mutex::new(None),
            installer: Semaphore::new(1),
            checked: OnceCell::new(),
            progress,
            integrity: Arc::default(),
            leases: Mutex::default(),
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
        let _permit = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(cancelled()),
            permit = self.installer.acquire() => permit.map_err(|_| unavailable())?,
        };
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
        let path = self.root.join("environments").join(&key);
        if !self.valid(&path, &key, cancel).await? {
            let result = self.prepare(requirements, &key, &path, cancel).await;
            if result.is_err() {
                self.report(requirements, Stage::Failed);
            }
            result?;
        }
        check(cancel)?;
        let lease = {
            let mut leases = self.leases.lock().expect("Python lease lock is valid");
            let lease = leases.get(&key).and_then(Weak::upgrade).unwrap_or_default();
            leases.insert(key.clone(), Arc::downgrade(&lease));
            lease
        };
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
        path: &Path,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<bool, Fault> {
        let path = path.to_owned();
        let runtime = self.runtime.root.clone();
        let key = key.to_owned();
        let cache = self.integrity.clone();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let mut cache = cache.lock().expect("Python integrity lock is valid");
            integrity::valid(&path, &runtime, &key, &mut cache, &cancel)
        })
        .await
        .map_err(|_| unavailable())?
    }

    fn report(&self, requirements: &Requirements, stage: Stage) {
        self.progress.send_replace(Some(Progress {
            package: requirements.package.name.clone(),
            stage,
        }));
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
        let leases = self
            .leases
            .lock()
            .expect("Python lease lock is valid")
            .clone();
        let root = self.root.clone();
        let current = current.to_owned();
        tokio::task::spawn_blocking(move || {
            for entry in std::fs::read_dir(root.join("environments")).map_err(storage)? {
                let entry = entry.map_err(storage)?;
                let Some(key) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if key == current
                    || references.contains(&key)
                    || !integrity::key(&key)
                    || leases.get(&key).and_then(Weak::upgrade).is_some()
                {
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
                    std::fs::remove_dir_all(path).map_err(storage)?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| unavailable())?
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
