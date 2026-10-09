use super::Rules;
use crate::code::network::Network;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, code};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

type Current = (Arc<Rules>, code::Rules);
pub(in crate::code) struct Registry {
    profile: Option<PathBuf>,
    network: Arc<Network>,
    current: RwLock<Option<Current>>,
    update: tokio::sync::Mutex<()>,
}

impl Registry {
    pub fn new(profile: Option<PathBuf>, network: Arc<Network>) -> Self {
        Self {
            profile,
            network,
            current: RwLock::new(None),
            update: Default::default(),
        }
    }
    pub fn get(&self) -> Result<Arc<Rules>, Fault> {
        Ok(self.current()?.0)
    }
    pub fn status(&self) -> Result<code::Rules, Fault> {
        Ok(self.current()?.1)
    }
    fn current(&self) -> Result<Current, Fault> {
        if let Some(current) = self
            .current
            .read()
            .expect("code rules lock is healthy")
            .clone()
        {
            return Ok(current);
        }
        let mut current = self.current.write().expect("code rules lock is healthy");
        if let Some(current) = current.as_ref() {
            return Ok(current.clone());
        }
        let selected = self.profile.as_ref().and_then(|profile| {
            let directory = profile.join("code/rules");
            if !directory.exists() {
                return None;
            }
            match cached(&directory) {
                Ok(current) => Some(current),
                Err(error) => {
                    eprintln!("cached language rules are unavailable; using the embedded snapshot: {error}");
                    None
                }
            }
        });
        let selected = match selected {
            Some(selected) => selected,
            None => (
                Rules::load()?,
                metadata(
                    &serde_json::from_slice::<Value>(include_bytes!("../upstream/SOURCE.json"))
                        .map_err(invalid)?,
                    false,
                )?,
            ),
        };
        *current = Some(selected.clone());
        Ok(selected)
    }

    pub async fn refresh(&self, stop: &CancellationToken) -> Result<code::Rules, Fault> {
        let _active = self
            .update
            .try_lock()
            .map_err(|_| Fault::new(ErrorCode::Busy, "language rules update is already running"))?;
        let profile = self.profile.as_ref().ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "language rules need a Node profile",
            )
        })?;
        let commit = self
            .network
            .json(
                "https://api.github.com/repos/helix-editor/helix/commits/master",
                stop,
            )
            .await?
            .get("sha")
            .and_then(Value::as_str)
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| Fault::invalid("Helix update has no immutable commit"))?
            .to_owned();
        let release = self
            .network
            .json(
                "https://api.github.com/repos/mason-org/mason-registry/releases/latest",
                stop,
            )
            .await?;
        let tag = release
            .get("tag_name")
            .and_then(Value::as_str)
            .filter(|tag| {
                !tag.is_empty()
                    && tag.len() <= 128
                    && tag
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            })
            .ok_or_else(|| Fault::invalid("Mason update has no literal release tag"))?
            .to_owned();
        let helix_url =
            format!("https://raw.githubusercontent.com/helix-editor/helix/{commit}/languages.toml");
        let mason_base =
            format!("https://github.com/mason-org/mason-registry/releases/download/{tag}");
        let mason_url = format!("{mason_base}/registry.json.zip");
        let helix = self
            .network
            .bytes(&helix_url, 16 * 1024 * 1024, stop)
            .await?;
        let mason = self
            .network
            .bytes(&mason_url, 16 * 1024 * 1024, stop)
            .await?;
        let checksums = self
            .network
            .bytes(&format!("{mason_base}/checksums.txt"), 1024 * 1024, stop)
            .await?;
        let checksum = std::str::from_utf8(&checksums)
            .map_err(invalid)?
            .lines()
            .find_map(|line| {
                let mut words = line.split_whitespace();
                let hash = words.next()?;
                let name = words.next()?.trim_start_matches('*');
                (name == "registry.json.zip").then(|| hash.to_owned())
            })
            .ok_or_else(|| Fault::invalid("Mason update has no registry checksum"))?;
        if checksum != digest(&mason) {
            return Err(Fault::invalid("Mason registry checksum did not match"));
        }
        let mut reference = self
            .network
            .json(
                &format!(
                    "https://api.github.com/repos/mason-org/mason-registry/git/ref/tags/{tag}"
                ),
                stop,
            )
            .await?
            .get("object")
            .cloned()
            .ok_or_else(|| Fault::invalid("Mason tag has no immutable reference"))?;
        if reference.get("type").and_then(Value::as_str) == Some("tag") {
            let url = reference
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| Fault::invalid("Mason annotated tag has no URL"))?;
            reference = self
                .network
                .json(url, stop)
                .await?
                .get("object")
                .cloned()
                .ok_or_else(|| Fault::invalid("Mason annotated tag has no target"))?;
        }
        let mason_commit = reference
            .get("sha")
            .and_then(Value::as_str)
            .filter(|sha| {
                reference.get("type").and_then(Value::as_str) == Some("commit")
                    && sha.len() == 40
                    && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .ok_or_else(|| Fault::invalid("Mason tag did not resolve to a commit"))?;
        let helix_license = self
            .network
            .bytes(
                &format!("https://raw.githubusercontent.com/helix-editor/helix/{commit}/LICENSE"),
                1024 * 1024,
                stop,
            )
            .await?;
        let mason_license = self
            .network
            .bytes(
                &format!("https://raw.githubusercontent.com/mason-org/mason-registry/{mason_commit}/LICENSE"),
                1024 * 1024,
                stop,
            )
            .await?;
        let source = json!({
            "version": 1,
            "retrieved": chrono::Utc::now().date_naive().to_string(),
            "helix": {
                "repository": "https://github.com/helix-editor/helix",
                "commit": commit,
                "url": helix_url,
                "sha256": digest(&helix),
                "license": "MPL-2.0",
                "license_sha256": digest(&helix_license),
            },
            "mason": {
                "repository": "https://github.com/mason-org/mason-registry",
                "tag": tag,
                "commit": mason_commit,
                "url": mason_url,
                "sha256": digest(&mason),
                "license": "Apache-2.0",
                "license_sha256": digest(&mason_license),
            },
        });
        let rules = Arc::new(
            Rules::parse(std::str::from_utf8(&helix).map_err(invalid)?, &mason).map_err(invalid)?,
        );
        if rules.languages.is_empty() || rules.servers.is_empty() || rules.packages.is_empty() {
            return Err(Fault::invalid("language update contains no usable rules"));
        }
        if stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "language rules update was cancelled",
            ));
        }
        let parent = profile.join("code");
        std::fs::create_dir_all(&parent).map_err(invalid)?;
        let temporary = tempfile::tempdir_in(&parent).map_err(invalid)?;
        let candidate = temporary.path().join("rules");
        std::fs::create_dir(&candidate).map_err(invalid)?;
        for (name, bytes) in [
            ("languages.toml", helix),
            ("registry.json.zip", mason),
            ("checksums.txt", checksums),
            ("helix-LICENSE", helix_license),
            ("mason-LICENSE", mason_license),
            (
                "SOURCE.json",
                serde_json::to_vec_pretty(&source).map_err(invalid)?,
            ),
        ] {
            std::fs::write(candidate.join(name), bytes).map_err(invalid)?;
        }
        let _ = cached(&candidate)?;
        let active = parent.join("rules");
        let previous = temporary.path().join("previous");
        if active.exists() {
            std::fs::rename(&active, &previous).map_err(invalid)?;
        }
        if let Err(error) = std::fs::rename(&candidate, &active) {
            if previous.exists() {
                std::fs::rename(&previous, &active).map_err(|rollback| {
                    Fault::new(
                        ErrorCode::OutcomeUnknown,
                        format!("language update could not restore its previous cache: {rollback}"),
                    )
                })?;
            }
            return Err(invalid(error));
        }
        let status = metadata(&source, true)?;
        *self.current.write().expect("code rules lock is healthy") = Some((rules, status.clone()));
        Ok(status)
    }
}

fn cached(path: &Path) -> Result<Current, Fault> {
    let source: Value =
        serde_json::from_slice(&read(path, "SOURCE.json", 64 * 1024)?).map_err(invalid)?;
    if source.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(Fault::invalid("unsupported language snapshot version"));
    }
    let helix = read(path, "languages.toml", 16 * 1024 * 1024)?;
    let mason = read(path, "registry.json.zip", 16 * 1024 * 1024)?;
    for (name, bytes) in [("helix", &helix), ("mason", &mason)] {
        if source
            .pointer(&format!("/{name}/sha256"))
            .and_then(Value::as_str)
            != Some(digest(bytes).as_str())
        {
            return Err(Fault::invalid(
                "cached language snapshot checksum did not match",
            ));
        }
        let license = read(path, &format!("{name}-LICENSE"), 1024 * 1024)?;
        if source
            .pointer(&format!("/{name}/license_sha256"))
            .and_then(Value::as_str)
            != Some(digest(&license).as_str())
        {
            return Err(Fault::invalid(
                "cached language license checksum did not match",
            ));
        }
    }
    Ok((
        Arc::new(
            Rules::parse(std::str::from_utf8(&helix).map_err(invalid)?, &mason).map_err(invalid)?,
        ),
        metadata(&source, true)?,
    ))
}
fn read(path: &Path, name: &str, limit: usize) -> Result<Vec<u8>, Fault> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path.join(name))
        .map_err(invalid)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > limit {
        return Err(Fault::invalid(
            "cached language snapshot exceeds the size limit",
        ));
    }
    Ok(bytes)
}
fn metadata(source: &Value, cached: bool) -> Result<code::Rules, Fault> {
    Ok(code::Rules {
        helix_commit: source
            .pointer("/helix/commit")
            .and_then(Value::as_str)
            .ok_or_else(|| Fault::invalid("language snapshot has no Helix commit"))?
            .into(),
        mason_tag: source
            .pointer("/mason/tag")
            .and_then(Value::as_str)
            .ok_or_else(|| Fault::invalid("language snapshot has no Mason release"))?
            .into(),
        retrieved: source
            .get("retrieved")
            .and_then(Value::as_str)
            .ok_or_else(|| Fault::invalid("language snapshot has no retrieval date"))?
            .into(),
        cached,
    })
}
pub(in crate::code) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn invalid(error: impl std::fmt::Display) -> Fault {
    Fault::invalid(error.to_string())
}

#[cfg(test)]
mod tests;
