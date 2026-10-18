use super::{
    FILE_BYTES, Failure, MANIFEST_BYTES, PACKAGE_BYTES, Result, Selection, Service, catalog,
    transfer,
};
use crate::theme::package::{Loaded, market::SOURCE_FILE};
use sailry_link::CancellationToken;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{collections::BTreeSet, path::Path};

const REPOSITORY: &str = "https://github.com/sailry/sailry-themes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Installed {
    pub id: String,
    pub version: Option<String>,
    pub digest: String,
    pub official: bool,
    pub commit: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Origin {
    version: u32,
    repository: String,
    commit: String,
    package_version: String,
    digest: String,
}

pub(crate) struct Prepared {
    pub loaded: Loaded,
    selection: Selection,
    expected: Option<Installed>,
}

pub(crate) struct InstalledResult {
    pub loaded: Loaded,
    pub installed: Installed,
}

impl catalog::Entry {
    pub fn update_available(&self, installed: &Installed) -> bool {
        installed.official
            && installed.id == self.id
            && self.downloadable()
            && installed
                .version
                .as_ref()
                .and_then(|version| Version::parse(version).ok())
                .zip(Version::parse(&self.package_version).ok())
                .is_some_and(|(current, candidate)| candidate.cmp_precedence(&current).is_gt())
    }
}

impl Service {
    pub async fn installed(&self, id: &str) -> Result<Option<Installed>> {
        let root = self.root.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || inspect(&root, &id))
            .await
            .map_err(|_| Failure::Io("theme inspection failed".into()))?
    }

    pub async fn preview(
        &self,
        selection: &Selection,
        mode: &str,
        stop: &CancellationToken,
    ) -> Result<Option<Vec<u8>>> {
        selection.entry.validate()?;
        if !catalog::commit(&selection.commit) {
            return Err(Failure::Invalid);
        }
        let Some(path) = selection.entry.previews.get(mode) else {
            return Ok(None);
        };
        let Some(resource) = selection
            .entry
            .resources
            .iter()
            .find(|resource| &resource.path == path)
        else {
            // A preview without its own integrity descriptor can wait for strict installation.
            return Ok(None);
        };
        let url = self
            .source
            .resource(&selection.commit, &selection.entry, path);
        let bytes = transfer::body(&self.client, &url, resource.bytes, stop).await?;
        verify(resource, &bytes)?;
        Ok(Some(bytes))
    }

    pub async fn download(
        &self,
        selection: Selection,
        stop: &CancellationToken,
    ) -> Result<Prepared> {
        selection.entry.validate()?;
        if !catalog::commit(&selection.commit) || !selection.entry.downloadable() {
            return Err(Failure::Invalid);
        }
        let expected = self.installed(&selection.entry.id).await?;
        if expected
            .as_ref()
            .is_some_and(|installed| !selection.entry.update_available(installed))
        {
            return Err(Failure::Conflict);
        }
        let directory = tempfile::tempdir()?;
        let bytes = self
            .resource(&selection, "theme.json", MANIFEST_BYTES, stop)
            .await?;
        let manifest =
            crate::theme::package::market::manifest(&bytes).map_err(|_| Failure::Invalid)?;
        if manifest.id != selection.entry.id
            || manifest.package_version.as_deref() != Some(selection.entry.package_version.as_str())
        {
            return Err(Failure::Invalid);
        }
        let paths = manifest
            .light
            .assets
            .values()
            .chain(manifest.dark.assets.values())
            .map(|asset| asset.path.as_str())
            .collect::<BTreeSet<_>>();
        if selection.entry.resources.iter().any(|resource| {
            resource.path != "theme.json" && !paths.contains(resource.path.as_str())
        }) {
            return Err(Failure::Invalid);
        }
        let mut total = bytes.len();
        std::fs::write(directory.path().join("theme.json"), bytes)?;
        for relative in paths {
            let remaining = PACKAGE_BYTES.checked_sub(total).ok_or(Failure::Invalid)?;
            let bytes = self
                .resource(&selection, relative, FILE_BYTES.min(remaining), stop)
                .await?;
            total = total.checked_add(bytes.len()).ok_or(Failure::Invalid)?;
            let path = directory.path().join(relative);
            std::fs::create_dir_all(path.parent().ok_or(Failure::Invalid)?)?;
            std::fs::write(path, bytes)?;
        }
        if stop.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        let loaded = tokio::task::spawn_blocking(move || Loaded::read(directory.path(), true))
            .await
            .map_err(|_| Failure::Io("theme validation failed".into()))?
            .map_err(|_| Failure::Invalid)?;
        if loaded.manifest.id != selection.entry.id
            || loaded.manifest.package_version.as_deref()
                != Some(selection.entry.package_version.as_str())
            || loaded.digest() != selection.entry.digest
        {
            return Err(Failure::Invalid);
        }
        if stop.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        Ok(Prepared {
            loaded,
            selection,
            expected,
        })
    }

    async fn resource(
        &self,
        selection: &Selection,
        path: &str,
        limit: usize,
        stop: &CancellationToken,
    ) -> Result<Vec<u8>> {
        let hint = selection
            .entry
            .resources
            .iter()
            .find(|resource| resource.path == path);
        let url = self
            .source
            .resource(&selection.commit, &selection.entry, path);
        let bytes = transfer::body(
            &self.client,
            &url,
            hint.map_or(limit, |resource| resource.bytes.min(limit)),
            stop,
        )
        .await?;
        if let Some(resource) = hint {
            verify(resource, &bytes)?;
        }
        Ok(bytes)
    }

    pub async fn install(
        &self,
        mut prepared: Prepared,
        stop: &CancellationToken,
    ) -> Result<InstalledResult> {
        let publication = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err(Failure::Cancelled),
            guard = self.publication.clone().lock_owned() => guard,
        };
        if self.installed(&prepared.selection.entry.id).await? != prepared.expected {
            return Err(Failure::Conflict);
        }
        if prepared.loaded.digest() != prepared.selection.entry.digest {
            return Err(Failure::Invalid);
        }
        let origin = Origin {
            version: 1,
            repository: REPOSITORY.into(),
            commit: prepared.selection.commit.clone(),
            package_version: prepared.selection.entry.package_version.clone(),
            digest: prepared.selection.entry.digest.clone(),
        };
        prepared.loaded.market_source(Some(
            serde_json::to_vec(&origin).map_err(|_| Failure::Invalid)?,
        ));
        if stop.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        // Once publication starts, return its actual result rather than racing it
        // against cancellation and reporting failure after a successful replacement.
        let root = self.root.clone();
        let stop = stop.clone();
        tokio::task::spawn_blocking(move || {
            let _publication = publication;
            if stop.is_cancelled() {
                return Err(Failure::Cancelled);
            }
            prepared
                .loaded
                .install(&root)
                .map_err(|error| Failure::Publication {
                    message: error.to_string(),
                    recovery: error.recovery,
                })?;
            prepared.loaded.market_source(None);
            Ok(InstalledResult {
                loaded: prepared.loaded,
                installed: Installed {
                    id: prepared.selection.entry.id,
                    version: Some(origin.package_version),
                    digest: origin.digest,
                    official: true,
                    commit: Some(origin.commit),
                },
            })
        })
        .await
        .map_err(|_| Failure::Io("theme publication failed".into()))?
    }
}

fn verify(resource: &catalog::Resource, bytes: &[u8]) -> Result<()> {
    if resource.bytes != bytes.len()
        || resource.digest != format!("sha256:{:x}", Sha256::digest(bytes))
    {
        return Err(Failure::Invalid);
    }
    Ok(())
}

fn inspect(root: &Path, id: &str) -> Result<Option<Installed>> {
    if !crate::theme::package::valid_id(id) || id == "builtin" {
        return Err(Failure::Invalid);
    }
    let path = root.join(id);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Failure::Conflict);
    }
    let loaded = Loaded::read(&path, true).map_err(|_| Failure::Invalid)?;
    if loaded.manifest.id != id {
        return Err(Failure::Invalid);
    }
    let digest = loaded.digest();
    let origin = transfer::read(&path.join(SOURCE_FILE), MANIFEST_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Origin>(&bytes).ok())
        .filter(|origin| {
            origin.version == 1
                && origin.repository == REPOSITORY
                && catalog::commit(&origin.commit)
                && origin.digest == digest
                && loaded.manifest.package_version.as_ref() == Some(&origin.package_version)
        });
    Ok(Some(Installed {
        id: id.into(),
        version: loaded.manifest.package_version,
        digest,
        official: origin.is_some(),
        commit: origin.map(|origin| origin.commit),
    }))
}
