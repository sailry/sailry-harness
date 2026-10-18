//! Skills and plugins share Git acquisition, without changing their package validators.
use super::github::cancelled;
use super::{Host, io_error};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::skills::{Resolved, Source},
};
use std::fs::File;
mod git;
pub(super) mod source;

pub(super) const LICENSES: [&str; 5] =
    ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING", "NOTICE"];

impl Host {
    pub(super) async fn source_manifest(
        &self,
        selected: &source::Selection,
        stop: CancellationToken,
    ) -> Result<(Vec<u8>, Resolved), Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if self.github.endpoint.is_some() {
            let (file, resolved) = self
                .source_archive(selected, None, stop.clone(), None)
                .await?;
            let path = selected.path.clone();
            return tokio::task::spawn_blocking(move || {
                manifest_archive(file, &path, &stop).map(|bytes| (bytes, resolved))
            })
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git source worker is unavailable"))?;
        }
        let selected = source::Selection {
            repository: selected.repository.clone(),
            git_ref: selected.git_ref.clone(),
            path: selected.path.clone(),
        };
        tokio::task::spawn_blocking(move || git::manifest(&selected, &stop))
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git source worker is unavailable"))?
    }

    pub(super) async fn source_archive(
        &self,
        selected: &source::Selection,
        commit: Option<&str>,
        stop: CancellationToken,
        reporter: Option<crate::installation::Reporter>,
    ) -> Result<(File, Resolved), Fault> {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        if let Some(reporter) = &reporter {
            reporter(sailry_protocol::installation::Phase::Downloading, None);
        }
        #[cfg(any(test, feature = "test-support"))]
        if self.github.endpoint.is_some() {
            // Existing HTTP fixtures replace acquisition explicitly; production never falls back.
            let selected = super::github::source::Selection::parse(&Source {
                repository: selected.repository.clone(),
                git_ref: Some(selected.git_ref.clone()),
                path: Some(selected.path.clone()),
            })?;
            let fixed = commit.or_else(|| {
                super::github::source::commit(&selected.git_ref)
                    .then_some(selected.git_ref.as_str())
            });
            let resolved = if let Some(commit) = fixed {
                Resolved {
                    repository: selected.repository.clone(),
                    git_ref: selected.git_ref.clone(),
                    commit: commit.into(),
                }
            } else {
                self.github.resolve(&selected, &stop).await?
            };
            return Ok((
                self.github
                    .archive(&selected, &resolved.commit, &stop)
                    .await?,
                resolved,
            ));
        }
        let selected = source::Selection {
            repository: selected.repository.clone(),
            git_ref: selected.git_ref.clone(),
            path: selected.path.clone(),
        };
        let commit = commit.map(str::to_owned);
        tokio::task::spawn_blocking(move || {
            git::acquire(&selected, commit.as_deref(), &stop, reporter)
        })
        .await
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git source worker is unavailable"))?
    }
}

#[cfg(any(test, feature = "test-support"))]
fn manifest_archive(
    mut file: File,
    path: &str,
    stop: &CancellationToken,
) -> Result<Vec<u8>, Fault> {
    use sailry_protocol::plugin::MAX_PACKAGE_BYTES;
    use std::{
        collections::BTreeSet,
        io::{Read, Seek, SeekFrom},
    };
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| Fault::invalid("invalid plugin ZIP"))?;
    if archive.len() > 10_000 {
        return Err(Fault::invalid("plugin ZIP has too many entries"));
    }
    let mut names = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut links = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        if stop.is_cancelled() {
            return Err(cancelled());
        }
        let entry = archive
            .by_index(index)
            .map_err(|_| Fault::invalid("invalid plugin ZIP entry"))?;
        let name = entry.name().trim_end_matches('/').to_owned();
        let parts = crate::files::path::components(&name, false)?;
        if parts.len() > 32
            || !names.insert(name.clone())
            || entry.enclosed_name().is_none()
            || entry.encrypted()
            || (!entry.is_symlink()
                && entry
                    .unix_mode()
                    .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o040000 | 0o100000)))
        {
            return Err(Fault::invalid("unsupported plugin ZIP entry"));
        }
        roots.insert(parts[0].to_owned());
        if entry.is_symlink() {
            links.push(name);
        } else if !entry.is_dir() {
            total = total
                .checked_add(entry.size())
                .filter(|size| *size <= MAX_PACKAGE_BYTES)
                .ok_or_else(|| Fault::invalid("plugin ZIP exceeds the expanded size limit"))?;
        }
    }
    let prefix = if names.contains("plugin.json") || roots.len() != 1 {
        String::new()
    } else {
        format!("{}/", roots.first().unwrap())
    };
    let name = if path.is_empty() {
        format!("{prefix}plugin.json")
    } else {
        format!("{prefix}{path}/plugin.json")
    };
    if links
        .iter()
        .any(|link| name == *link || name.starts_with(&format!("{link}/")))
    {
        return Err(Fault::invalid(
            "selected plugin manifest must be a regular file",
        ));
    }
    let mut entry = archive
        .by_name(&name)
        .map_err(|_| Fault::invalid("plugin.json is required"))?;
    if entry.is_dir() || entry.is_symlink() || entry.size() > 64 * 1024 {
        return Err(Fault::invalid(
            "selected plugin manifest must be a bounded regular file",
        ));
    }
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    let size = entry.size();
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Fault::invalid("invalid plugin ZIP data"))?;
    if bytes.len() as u64 != size {
        return Err(Fault::invalid("plugin ZIP size mismatch"));
    }
    Ok(bytes)
}
