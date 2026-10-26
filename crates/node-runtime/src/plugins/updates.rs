//! Inspect sources without installing packages or changing their configuration.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    Origin,
    catalog::Entry,
    skills::Source,
    updates::{Blocked, Candidate, Report},
};

pub(crate) struct Catalog {
    entries: Vec<Entry>,
}

impl Host {
    pub(crate) async fn update_catalog(
        &self,
        packages: &[Info],
        force: bool,
        stop: &CancellationToken,
    ) -> Option<Result<Catalog, Fault>> {
        if packages.iter().any(catalog::official::owns) {
            Some(
                catalog::official::fetch(self, force, stop)
                    .await
                    .map(|entries| Catalog { entries }),
            )
        } else {
            None
        }
    }

    pub(crate) fn inspect(
        &self,
        worktree: &Path,
        path: &str,
        name: &str,
        stop: CancellationToken,
    ) -> Result<Info, Fault> {
        let (source, manifest) = self.inspect_manifest(worktree, path, name)?;
        inspect_directory(&source, manifest, stop)
    }

    fn inspect_newer(
        &self,
        worktree: &Path,
        path: &str,
        name: &str,
        current: Option<&str>,
        stop: CancellationToken,
    ) -> Result<Option<Info>, Fault> {
        let (source, manifest) = self.inspect_manifest(worktree, path, name)?;
        if !version(manifest.version.as_deref())?
            .cmp_precedence(&version(current)?)
            .is_gt()
        {
            return Ok(None);
        }
        inspect_directory(&source, manifest, stop).map(Some)
    }

    fn inspect_manifest(
        &self,
        worktree: &Path,
        path: &str,
        name: &str,
    ) -> Result<(cap_std::fs::Dir, manifest::Manifest), Fault> {
        use crate::files::path::{components, descend, root};
        validate_name(name)?;
        let resolved = worktree.join(path).canonicalize().map_err(io_error)?;
        if self
            .profile
            .as_ref()
            .is_some_and(|profile| resolved.starts_with(profile) || profile.starts_with(&resolved))
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "plugin source overlaps Node storage",
            ));
        }
        let source = descend(root(worktree)?, &components(path, true)?)?;
        let bytes = package::read(&source, "plugin.json", manifest::MAX_BYTES)?
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin.json is required"))?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != name {
            return Err(sailry_protocol::Fault::invalid(
                "plugin name does not match the selected package",
            ));
        }
        Ok((source, manifest))
    }

    pub(crate) async fn check_update(
        &self,
        current: Info,
        root: Option<PathBuf>,
        stop: CancellationToken,
        catalog: Option<&Result<Catalog, Fault>>,
    ) -> Result<Report, Fault> {
        if current.skill.is_none() && version(current.summary.version.as_deref()).is_err() {
            return Ok(Report {
                blocked: None,
                current: current.summary,
                available: None,
                manual: true,
            });
        }
        if catalog::official::owns(&current) {
            let bundled = match Self::bundled_version(&current.summary.name) {
                Ok(value) => Some(version(value.as_deref())?),
                Err(error) if error.code == ErrorCode::NotFound => None,
                Err(error) => return Err(error),
            };
            let installed = version(current.summary.version.as_deref())?;
            let bundled = bundled.filter(|available| {
                available.cmp_precedence(&installed).is_gt()
                    && Self::bundled_compatibility(&current.summary.name)
                        .is_ok_and(|compatibility| compatibility.is_compatible())
            });
            let entry = catalog
                .ok_or_else(|| Fault::invalid("official plugin catalog is unavailable"))
                .and_then(|catalog| catalog.as_ref().map_err(Clone::clone))
                .and_then(|catalog| {
                    catalog
                        .entries
                        .iter()
                        .find(|entry| entry.id == current.summary.name)
                        .ok_or_else(|| {
                            Fault::new(ErrorCode::NotFound, "official plugin is unavailable")
                        })
                })
                .and_then(|entry| {
                    let available = version(entry.version.as_deref())?;
                    Ok((entry, available))
                });
            let blocked = entry
                .as_ref()
                .ok()
                .and_then(|(entry, _)| blocked_entry(entry, &installed));
            let available = match entry {
                Ok((entry, available))
                    if entry.compatibility.is_compatible()
                        && available.cmp_precedence(&installed).is_gt()
                        && bundled
                            .as_ref()
                            .is_none_or(|bundle| !bundle.cmp_precedence(&available).is_gt()) =>
                {
                    Some(Candidate::Official(Box::new(entry.clone())))
                }
                Ok(_) => self
                    .bundled_update(&current, stop)
                    .await?
                    .map(|info| Candidate::Package(Box::new(info))),
                Err(error)
                    if bundled.is_some()
                        && (catalog::official::unavailable(&error)
                            || error.code == ErrorCode::InvalidRequest) =>
                {
                    self.bundled_update(&current, stop)
                        .await?
                        .map(|info| Candidate::Package(Box::new(info)))
                }
                Err(error) => return Err(error),
            };
            return Ok(Report {
                blocked,
                available,
                current: current.summary,
                manual: false,
            });
        }
        let current_version = current.summary.version.clone();
        let name = current.summary.name.clone();
        let (mut candidate, origin) = match &current.origin {
            Some(Origin::Worktree { worktree, path }) => {
                let origin = Origin::Worktree {
                    worktree: *worktree,
                    path: path.clone(),
                };
                let path = path.clone();
                let root = root.ok_or_else(|| {
                    sailry_protocol::Fault::invalid("plugin source worktree is unavailable")
                })?;
                let host = self.clone();
                let candidate = tokio::task::spawn_blocking(move || {
                    host.inspect_newer(&root, &path, &name, current_version.as_deref(), stop)
                })
                .await
                .map_err(|_| github::unavailable())??;
                let Some(candidate) = candidate else {
                    return Ok(Report {
                        blocked: None,
                        current: current.summary,
                        available: None,
                        manual: false,
                    });
                };
                (candidate, origin)
            }
            Some(Origin::Registry { source }) => {
                let Some(candidate) = self.inspect_registry_skill(source, &name, stop).await?
                else {
                    return Ok(Report {
                        blocked: None,
                        current: current.summary,
                        available: None,
                        manual: false,
                    });
                };
                let Some(sailry_protocol::plugin::skills::Provenance::Registry { registry }) =
                    &candidate.skill
                else {
                    return Err(Fault::invalid("registry skill provenance is unavailable"));
                };
                let origin = Origin::Registry {
                    source: registry.clone(),
                };
                (candidate, origin)
            }
            Some(Origin::Online { source, path }) => {
                let source = Source {
                    repository: source.repository.clone(),
                    git_ref: Some(source.git_ref.clone()),
                    path: Some(path.clone()),
                };
                if current.skill.is_some() {
                    let candidate = self.inspect_skill(&source, &name, stop).await?;
                    let provenance = candidate
                        .skill
                        .as_ref()
                        .and_then(|value| value.git())
                        .expect("inspected Git skill provenance");
                    let origin = Origin::Online {
                        source: provenance.source.clone(),
                        path: provenance.path.clone(),
                    };
                    (candidate, origin)
                } else {
                    let Some(selected) = self
                        .inspect_newer_source(&source, &current.summary, stop)
                        .await?
                    else {
                        return Ok(Report {
                            blocked: None,
                            current: current.summary,
                            available: None,
                            manual: false,
                        });
                    };
                    (
                        selected.info,
                        Origin::Online {
                            source: selected.source,
                            path: selected.path,
                        },
                    )
                }
            }
            _ => {
                return Ok(Report {
                    blocked: None,
                    current: current.summary,
                    available: None,
                    manual: true,
                });
            }
        };
        if candidate.summary.name != current.summary.name {
            return Err(sailry_protocol::Fault::invalid(
                "plugin update has a different identity",
            ));
        }
        candidate.origin = Some(origin);
        let blocked = blocked_info(&candidate);
        let compatible = blocked.is_none();
        Ok(Report {
            blocked,
            available: (compatible
                && (current.skill.is_none() || candidate.summary.digest != current.summary.digest))
                .then_some(Candidate::Package(Box::new(candidate))),
            current: current.summary,
            manual: false,
        })
    }

    async fn bundled_update(
        &self,
        current: &Info,
        stop: CancellationToken,
    ) -> Result<Option<Info>, Fault> {
        match Self::bundled_newer(&current.summary.name, current.summary.version.as_deref()) {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
        let host = self.clone();
        let name = current.summary.name.clone();
        let mut bundled =
            match tokio::task::spawn_blocking(move || host.inspect_bundled(&name, stop))
                .await
                .map_err(|_| github::unavailable())?
            {
                Ok(bundled) => bundled,
                Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
        bundled.origin = Some(Origin::Bundled);
        Ok(Some(bundled))
    }
}

fn inspect_directory(
    source: &cap_std::fs::Dir,
    manifest: manifest::Manifest,
    stop: CancellationToken,
) -> Result<Info, Fault> {
    let mut tree = package::Tree::default();
    tree.stop = stop;
    tree.walk(source, None, "", 0)?;
    describe(source, manifest, tree.digest(), tree.issues)
}

pub(super) fn version(value: Option<&str>) -> Result<semver::Version, Fault> {
    value
        .and_then(|value| semver::Version::parse(value).ok())
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin semantic version is unavailable"))
}

fn blocked_info(info: &Info) -> Option<Blocked> {
    Some(Blocked {
        version: info.summary.version.clone()?,
        required: info.summary.compatibility.required()?.into(),
        release_notes: info.release_notes.clone(),
    })
}

fn blocked_entry(entry: &Entry, current: &semver::Version) -> Option<Blocked> {
    if let Some(version) = entry.versions.iter().find(|version| {
        !version.compatibility.is_compatible()
            && semver::Version::parse(&version.version)
                .is_ok_and(|version| version.cmp_precedence(current).is_gt())
    }) {
        return Some(Blocked {
            version: version.version.clone(),
            required: version.compatibility.required()?.into(),
            release_notes: version.release_notes.clone(),
        });
    }
    if version(entry.version.as_deref())
        .ok()?
        .cmp_precedence(current)
        .is_gt()
    {
        Some(Blocked {
            version: entry.version.clone()?,
            required: entry.compatibility.required()?.into(),
            release_notes: entry.release_notes.clone(),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
