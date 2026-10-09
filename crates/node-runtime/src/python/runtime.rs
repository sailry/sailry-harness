use super::{VERSION, integrity, process, storage};
use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    python_version: String,
    target: String,
    source: Source,
    files: Vec<integrity::File>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    url: String,
    sha256: String,
}

struct Verification {
    identity: String,
    files: Vec<integrity::File>,
    cache: Mutex<integrity::Cache>,
}

// Repeated isolated Nodes can share read-only asset verification. Entries carry
// no profile, settings, credentials, processes or environment lifecycle state.
static VERIFIED: LazyLock<Mutex<BTreeMap<PathBuf, Arc<Verification>>>> =
    LazyLock::new(Mutex::default);

#[derive(Clone)]
pub(crate) struct Runtime {
    pub(super) root: PathBuf,
    pub(super) python: PathBuf,
    pub(super) identity: String,
    verification: Arc<Verification>,
}

impl Runtime {
    pub(crate) fn bundled() -> Result<Self, Fault> {
        let executable = std::env::current_exe().map_err(storage)?;
        let directory = executable.parent().ok_or_else(super::unavailable)?;
        Self::open(&directory.join("python"))
    }

    pub(crate) fn open(directory: &Path) -> Result<Self, Fault> {
        if !directory.is_absolute() || !directory.is_dir() {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "bundled Python runtime is unavailable",
            ));
        }
        let root = directory.canonicalize().map_err(storage)?;
        let path = root.join(if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3.12"
        });
        let python = path.canonicalize().map_err(storage)?;
        if !python.starts_with(&root) || !python.is_file() {
            return Err(Fault::invalid(
                "bundled Python interpreter points outside its runtime",
            ));
        }
        let marker = root.join(if cfg!(windows) {
            "Lib/EXTERNALLY-MANAGED"
        } else {
            "lib/python3.12/EXTERNALLY-MANAGED"
        });
        let metadata = marker.symlink_metadata().map_err(storage)?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err(Fault::invalid("bundled Python must be externally managed"));
        }
        let receipt_path = root.join("python-runtime.json");
        let metadata = receipt_path.symlink_metadata().map_err(storage)?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 16 * 1024 * 1024 {
            return Err(Fault::invalid("bundled Python receipt is invalid"));
        }
        let content = std::fs::read(&receipt_path).map_err(storage)?;
        let identity = blake3::hash(&content).to_hex().to_string();
        let receipt: Receipt = serde_json::from_slice(&content).map_err(storage)?;
        let source = reqwest::Url::parse(&receipt.source.url).map_err(storage)?;
        if receipt.version != 1
            || receipt.python_version != VERSION
            || receipt.target.is_empty()
            || source.scheme() != "https"
            || source.host_str().is_none()
            || !source.username().is_empty()
            || source.password().is_some()
            || !integrity::key(&receipt.source.sha256)
            || !receipt
                .files
                .iter()
                .any(|file| root.join(&file.path) == python && file.link.is_none())
        {
            return Err(Fault::invalid(
                "bundled Python receipt does not match its runtime",
            ));
        }
        let verification = {
            let mut verified = VERIFIED
                .lock()
                .expect("bundled Python verification lock is valid");
            match verified
                .get(&root)
                .filter(|entry| entry.identity == identity)
            {
                Some(entry) => entry.clone(),
                None => {
                    let verification = Arc::new(Verification {
                        identity: identity.clone(),
                        files: receipt.files,
                        cache: Mutex::default(),
                    });
                    if verified.len() >= 8 {
                        verified.pop_first();
                    }
                    verified.insert(root.clone(), verification.clone());
                    verification
                }
            }
        };
        let identity = blake3::hash(
            &serde_json::to_vec(&(
                identity,
                root.to_str()
                    .ok_or_else(|| Fault::invalid("bundled Python path is not Unicode"))?,
            ))
            .map_err(storage)?,
        )
        .to_hex()
        .to_string();
        let runtime = Self {
            root,
            python,
            identity,
            verification,
        };
        runtime.verify(&CancellationToken::new())?;
        Ok(runtime)
    }

    pub(super) async fn check(
        &self,
        environment: &Environment,
        cancel: &CancellationToken,
    ) -> Result<(), Fault> {
        process::run(
            &self.python,
            &["-I".into(), "-c".into(), format!("import sys,venv,pip; assert sys.version.split()[0] == '{VERSION}', 'bundled Python version mismatch'")],
            &self.root,
            environment,
            &[],
            cancel,
        ).await
    }

    pub(super) fn verify(&self, cancel: &CancellationToken) -> Result<(), Fault> {
        let mut cache = self
            .verification
            .cache
            .lock()
            .expect("bundled Python integrity lock is valid");
        if !integrity::verify(
            &self.root,
            &self.root,
            &self.verification.files,
            &mut cache,
            cancel,
        )? {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "bundled Python changed; reinstall the application",
            ));
        }
        Ok(())
    }
}

pub(super) fn python(directory: &Path) -> PathBuf {
    directory.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    })
}
