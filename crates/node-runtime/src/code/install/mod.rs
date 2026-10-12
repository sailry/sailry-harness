use super::{network::Network, rules::Registry};
use crate::installation::Reporter;
use sailry_link::CancellationToken;
use sailry_protocol::installation::Phase;
use sailry_protocol::{ErrorCode, Fault, code};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

mod archive;
mod plan;
mod toolchain;
pub(super) use plan::{package, version};
pub(super) fn validate_package(package: &str, version: &str) -> Result<(), Fault> {
    plan::component(package)?;
    plan::component(version)
}
#[cfg(any(test, feature = "test-support"))]
pub(super) use plan::fixture;

struct Job {
    stop: CancellationToken,
    state: code::Installation,
}
pub(super) struct Manager {
    profile: Option<PathBuf>,
    environment: Arc<crate::process::Environment>,
    rules: Arc<Registry>,
    pub(super) network: Arc<Network>,
    jobs: Mutex<BTreeMap<String, Job>>,
}

impl Manager {
    pub fn new(
        profile: Option<PathBuf>,
        environment: Arc<crate::process::Environment>,
        rules: Arc<Registry>,
        network: Arc<Network>,
    ) -> Self {
        Self {
            profile,
            environment,
            rules,
            network,
            jobs: Default::default(),
        }
    }
    pub fn validate(&self, install: &code::Install) -> Result<(), Fault> {
        plan::prepare(self.rules.get()?.as_ref(), install, &self.environment).map(|_| ())
    }

    pub fn list(&self) -> Result<Vec<code::Installation>, Fault> {
        let mut states = BTreeMap::new();
        if let Some(profile) = &self.profile {
            let rules = self.rules.get()?;
            let root = profile.join("tools/lsp");
            if root.exists() {
                for package in std::fs::read_dir(root).map_err(invalid)? {
                    let package = package.map_err(invalid)?;
                    if !package.file_type().map_err(invalid)?.is_dir() {
                        continue;
                    }
                    let name = package.file_name();
                    let Some(name) = name.to_str().filter(|name| plan::component(name).is_ok())
                    else {
                        continue;
                    };
                    let preferred = rules
                        .packages
                        .iter()
                        .find(|package| package.name == name)
                        .and_then(|package| version(package).ok());
                    if let Some((_, receipt)) = versions(profile, name, preferred.as_deref())?
                        .into_iter()
                        .next()
                    {
                        states.insert(
                            name.to_owned(),
                            code::Installation {
                                package: name.to_owned(),
                                status: code::InstallStatus::Ready,
                                progress: None,
                                receipt: Some(receipt),
                                error: None,
                            },
                        );
                    }
                }
            }
        }
        for (name, job) in self
            .jobs
            .lock()
            .expect("code install lock is healthy")
            .iter()
        {
            if job.state.status == code::InstallStatus::Ready && states.contains_key(name) {
                continue;
            }
            states.insert(name.clone(), job.state.clone());
        }
        Ok(states.into_values().collect())
    }

    pub fn cancel(&self, package: &str) -> Result<code::Installation, Fault> {
        plan::component(package)?;
        let jobs = self.jobs.lock().expect("code install lock is healthy");
        let job = jobs.get(package).ok_or_else(|| {
            Fault::new(ErrorCode::NotFound, "language installation is not running")
        })?;
        job.stop.cancel();
        Ok(job.state.clone())
    }

    pub fn shutdown(&self) {
        for job in self
            .jobs
            .lock()
            .expect("code install lock is healthy")
            .values()
        {
            job.stop.cancel();
        }
    }

    fn status(&self, package: &str, status: code::InstallStatus, progress: Option<String>) {
        if let Some(job) = self
            .jobs
            .lock()
            .expect("code install lock is healthy")
            .get_mut(package)
        {
            job.state.status = status;
            job.state.progress = progress;
        }
    }

    pub async fn install(
        self: &Arc<Self>,
        install: &code::Install,
        closed: &CancellationToken,
        reporter: Option<&Reporter>,
    ) -> Result<code::Installation, Fault> {
        let plan = plan::prepare(self.rules.get()?.as_ref(), install, &self.environment)?;
        let final_path = self.path(&install.package, &install.version)?;
        let stop = closed.child_token();
        {
            let mut jobs = self.jobs.lock().expect("code install lock is healthy");
            if jobs.get(&install.package).is_some_and(|job| {
                matches!(
                    job.state.status,
                    code::InstallStatus::Preparing
                        | code::InstallStatus::Downloading
                        | code::InstallStatus::Installing
                )
            }) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "this language package is already being installed",
                ));
            }
            if jobs.len() >= 256 && !jobs.contains_key(&install.package) {
                let stale = jobs
                    .iter()
                    .find(|(_, job)| {
                        matches!(
                            job.state.status,
                            code::InstallStatus::Ready
                                | code::InstallStatus::Failed
                                | code::InstallStatus::Cancelled
                        )
                    })
                    .map(|(name, _)| name.clone())
                    .ok_or_else(|| {
                        Fault::new(
                            ErrorCode::Busy,
                            "language installation capacity is exhausted",
                        )
                    })?;
                jobs.remove(&stale);
            }
            jobs.insert(
                install.package.clone(),
                Job {
                    stop: stop.clone(),
                    state: code::Installation {
                        package: install.package.clone(),
                        status: code::InstallStatus::Preparing,
                        progress: None,
                        receipt: None,
                        error: None,
                    },
                },
            );
        }
        let mut guard = Active {
            manager: Arc::downgrade(self),
            package: install.package.clone(),
            finished: false,
        };
        report(reporter, Phase::Preparing);
        let result = self.run(&plan, &final_path, &stop, reporter).await;
        let mut state = code::Installation {
            package: install.package.clone(),
            status: code::InstallStatus::Ready,
            progress: None,
            receipt: None,
            error: None,
        };
        match &result {
            Ok(receipt) => state.receipt = Some(receipt.clone()),
            Err(error) => {
                state.status = if error.code == ErrorCode::Cancelled {
                    code::InstallStatus::Cancelled
                } else {
                    code::InstallStatus::Failed
                };
                state.error = Some(error.message.clone());
            }
        }
        if let Some(job) = self
            .jobs
            .lock()
            .expect("code install lock is healthy")
            .get_mut(&install.package)
        {
            job.state = state.clone();
        }
        guard.finished = true;
        result.map(|_| state)
    }

    async fn run(
        &self,
        plan: &plan::Plan,
        final_path: &Path,
        stop: &CancellationToken,
        reporter: Option<&Reporter>,
    ) -> Result<code::Receipt, Fault> {
        if final_path.exists() {
            let stored = receipt(&final_path.join("receipt.json"))?;
            if stored.purl != plan.consent.purl || stored.revision != plan.consent.revision {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "installed language package differs from this version",
                ));
            }
            return Ok(stored);
        }
        let parent = final_path
            .parent()
            .ok_or_else(|| Fault::invalid("language install directory is unavailable"))?;
        std::fs::create_dir_all(parent).map_err(invalid)?;
        let temporary = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(parent)
            .map_err(invalid)?;
        let directory = temporary.path().canonicalize().map_err(invalid)?;
        let stage = directory.as_path();
        let mut downloads = Vec::new();
        match &plan.kind {
            plan::Kind::Archives { assets, release } => {
                report(reporter, Phase::Downloading);
                self.status(
                    &plan.consent.package,
                    code::InstallStatus::Downloading,
                    None,
                );
                let checksums = self.checksums(release.as_deref(), assets, stop).await?;
                for asset in assets {
                    report(reporter, Phase::Downloading);
                    self.status(
                        &plan.consent.package,
                        code::InstallStatus::Downloading,
                        Some(asset.name.clone()),
                    );
                    let (file, digest) = self
                        .network
                        .fetch(&asset.url, 512 * 1024 * 1024, stop)
                        .await?;
                    let expected = checksums.get(&asset.name);
                    report(reporter, Phase::Validating);
                    if expected.is_some_and(|expected| expected != &digest) {
                        return Err(Fault::invalid("language release checksum did not match"));
                    }
                    downloads.push(code::Download {
                        url: asset.url.clone(),
                        sha256: digest,
                        verified: expected.is_some(),
                    });
                    self.status(
                        &plan.consent.package,
                        code::InstallStatus::Installing,
                        Some(asset.name.clone()),
                    );
                    report(reporter, Phase::Installing);
                    let asset = asset.clone();
                    let root = stage.to_owned();
                    let stop = stop.clone();
                    tokio::task::spawn_blocking(move || {
                        archive::extract(file, &asset, &root, &stop)
                    })
                    .await
                    .map_err(invalid)??;
                }
                aliases(stage, final_path, &plan.binaries)?;
            }
            plan::Kind::Toolchain { .. } => {
                self.status(&plan.consent.package, code::InstallStatus::Installing, None);
                report(reporter, Phase::Installing);
                toolchain::install(plan, stage, final_path, &self.environment, stop).await?;
            }
        }
        if stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "language installation was cancelled",
            ));
        }
        let receipt = code::Receipt {
            package: plan.consent.package.clone(),
            version: plan.consent.version.clone(),
            purl: plan.consent.purl.clone(),
            revision: plan.consent.revision.clone(),
            downloads,
            installed_at: chrono::Utc::now().to_rfc3339(),
        };
        std::fs::write(
            stage.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).map_err(invalid)?,
        )
        .map_err(invalid)?;
        std::fs::rename(stage, final_path).map_err(invalid)?;
        Ok(receipt)
    }

    async fn checksums(
        &self,
        release: Option<&str>,
        assets: &[plan::Asset],
        stop: &CancellationToken,
    ) -> Result<BTreeMap<String, String>, Fault> {
        let mut checksums = BTreeMap::new();
        let Some(release) = release else {
            return Ok(checksums);
        };
        let release = match self.network.json(release, stop).await {
            Ok(release) => release,
            Err(error) if error.code != ErrorCode::Cancelled => return Ok(checksums),
            Err(error) => return Err(error),
        };
        let files = release
            .get("assets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for asset in assets {
            if let Some(hash) = files
                .iter()
                .find(|file| file.get("name").and_then(Value::as_str) == Some(&asset.name))
                .and_then(|file| file.get("digest"))
                .and_then(Value::as_str)
                .and_then(|value| value.strip_prefix("sha256:"))
                .filter(|hash| valid_digest(hash))
            {
                checksums.insert(asset.name.clone(), hash.to_ascii_lowercase());
            }
        }
        for file in files
            .iter()
            .filter(|file| {
                file.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| {
                        name.to_ascii_lowercase().contains("sha256")
                            || name.to_ascii_lowercase().contains("checksum")
                    })
            })
            .take(8)
        {
            let Some(url) = file.get("browser_download_url").and_then(Value::as_str) else {
                continue;
            };
            let text = self.network.bytes(url, 1024 * 1024, stop).await?;
            let text = std::str::from_utf8(&text).map_err(invalid)?;
            for line in text.lines() {
                let mut words = line.split_whitespace();
                let Some(hash) = words.next().filter(|hash| valid_digest(hash)) else {
                    continue;
                };
                if let Some(name) = words.next() {
                    let name = name.trim_start_matches('*');
                    if assets.iter().any(|asset| asset.name == name) {
                        checksums.insert(name.into(), hash.to_ascii_lowercase());
                    }
                } else if let Some(name) = file
                    .get("name")
                    .and_then(Value::as_str)
                    .and_then(|name| name.strip_suffix(".sha256"))
                    && assets.iter().any(|asset| asset.name == name)
                {
                    checksums.insert(name.into(), hash.to_ascii_lowercase());
                }
            }
        }
        Ok(checksums)
    }

    pub fn validate_uninstall(&self, package: &str, version: &str) -> Result<PathBuf, Fault> {
        let path = self.path(package, version)?;
        if self
            .jobs
            .lock()
            .expect("code install lock is healthy")
            .get(package)
            .is_some_and(|job| {
                matches!(
                    job.state.status,
                    code::InstallStatus::Preparing
                        | code::InstallStatus::Downloading
                        | code::InstallStatus::Installing
                )
            })
        {
            return Err(Fault::new(
                ErrorCode::Busy,
                "language package is being installed",
            ));
        }
        let current = receipt(&path.join("receipt.json"))?;
        if current.package != package || current.version != version {
            return Err(Fault::invalid(
                "language install receipt differs from its directory",
            ));
        }
        let path = path.canonicalize().map_err(invalid)?;
        let expected = self
            .profile
            .as_ref()
            .expect("managed profile was validated")
            .canonicalize()
            .map_err(invalid)?
            .join("tools/lsp")
            .join(package)
            .join(version);
        if path != expected {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "language package is outside managed storage",
            ));
        }
        Ok(path)
    }
    pub fn uninstall(&self, package: &str, version: &str) -> Result<(), Fault> {
        let path = self.validate_uninstall(package, version)?;
        std::fs::remove_dir_all(path).map_err(invalid)?;
        self.jobs
            .lock()
            .expect("code install lock is healthy")
            .remove(package);
        Ok(())
    }
    pub fn path(&self, package: &str, version: &str) -> Result<PathBuf, Fault> {
        plan::component(package)?;
        plan::component(version)?;
        Ok(self
            .profile
            .as_ref()
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::NotConfigured,
                    "language installation needs a Node profile",
                )
            })?
            .join("tools/lsp")
            .join(package)
            .join(version))
    }
}

fn report(reporter: Option<&Reporter>, phase: Phase) {
    if let Some(reporter) = reporter {
        reporter(phase, None);
    }
}

pub(in crate::code) fn versions(
    profile: &Path,
    package: &str,
    preferred: Option<&str>,
) -> Result<Vec<(PathBuf, code::Receipt)>, Fault> {
    plan::component(package)?;
    let entries = match std::fs::read_dir(profile.join("tools/lsp").join(package)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(invalid(error)),
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(invalid)?;
        if !entry.file_type().map_err(invalid)?.is_dir()
            || entry.file_name().to_string_lossy().starts_with('.')
            || !entry.path().join("receipt.json").is_file()
        {
            continue;
        }
        let record = (|| {
            let receipt = receipt(&entry.path().join("receipt.json"))?;
            if receipt.package != package
                || entry.file_name() != std::ffi::OsStr::new(&receipt.version)
                || plan::component(&receipt.version).is_err()
                || !valid_digest(&receipt.revision)
            {
                return Err(Fault::invalid(
                    "language install receipt differs from its directory",
                ));
            }
            let timestamp =
                chrono::DateTime::parse_from_rfc3339(&receipt.installed_at).map_err(invalid)?;
            Ok((entry.path(), receipt, timestamp))
        })();
        match record {
            Ok(record) => records.push(record),
            Err(error) => eprintln!("skipping invalid language install receipt: {error}"),
        }
    }
    records.sort_by(|left, right| {
        (preferred == Some(right.1.version.as_str()))
            .cmp(&(preferred == Some(left.1.version.as_str())))
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    Ok(records
        .into_iter()
        .map(|(path, receipt, _)| (path, receipt))
        .collect())
}

struct Active {
    manager: Weak<Manager>,
    package: String,
    finished: bool,
}
impl Drop for Active {
    fn drop(&mut self) {
        if !self.finished
            && let Some(manager) = self.manager.upgrade()
            && let Some(job) = manager
                .jobs
                .lock()
                .expect("code install lock is healthy")
                .get_mut(&self.package)
        {
            job.stop.cancel();
            job.state.status = code::InstallStatus::Cancelled;
            job.state.progress = None;
            job.state.error = Some("language installation was interrupted".into());
        }
    }
}
fn receipt(path: &Path) -> Result<code::Receipt, Fault> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(invalid)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > 64 * 1024 {
        return Err(Fault::invalid("language receipt exceeds the size limit"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}
fn aliases(
    stage: &Path,
    final_path: &Path,
    binaries: &BTreeMap<String, String>,
) -> Result<(), Fault> {
    let bin = stage.join("bin");
    std::fs::create_dir_all(&bin).map_err(invalid)?;
    for (name, relative) in binaries {
        let source = stage.join(relative);
        if !source.is_file() {
            return Err(Fault::invalid(
                "language release did not provide its declared binary",
            ));
        }
        let target = source.canonicalize().map_err(invalid)?;
        if !target.starts_with(stage) {
            return Err(Fault::invalid(
                "language release binary points outside its package",
            ));
        }
        archive::executable(&target, 0o755)?;
        #[cfg(unix)]
        {
            let alias = bin.join(name);
            if alias != source {
                std::os::unix::fs::symlink(final_path.join(relative), alias).map_err(invalid)?;
            }
        }
        #[cfg(windows)]
        {
            let alias = bin.join(format!("{name}.exe"));
            if alias != source {
                std::fs::copy(&source, alias).map_err(invalid)?;
            }
        }
    }
    Ok(())
}
fn valid_digest(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(error.to_string())
}

#[cfg(test)]
mod tests;
