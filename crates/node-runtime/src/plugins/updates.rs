//! Inspect sources without installing packages or changing their configuration.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    Origin,
    catalog::Entry,
    skills::Source,
    updates::{Candidate, Report},
};

impl Host {
    pub(crate) async fn update_catalog(
        &self,
        packages: &[Info],
        stop: &CancellationToken,
    ) -> Option<Result<Vec<Entry>, Fault>> {
        if packages.iter().any(catalog::official::owns) {
            Some(catalog::official::fetch(self, stop).await)
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
        let mut tree = package::Tree::default();
        tree.stop = stop;
        tree.walk(&source, None, "", 0)?;
        let bytes = package::read(&source, "plugin.json", 64 * 1024)?
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin.json is required"))?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != name {
            return Err(sailry_protocol::Fault::invalid(
                "plugin name does not match the selected package",
            ));
        }
        describe(&source, manifest, tree.digest(), tree.issues)
    }

    pub(crate) async fn check_update(
        &self,
        current: Info,
        root: Option<PathBuf>,
        stop: CancellationToken,
        catalog: Option<&Result<Vec<Entry>, Fault>>,
    ) -> Result<Report, Fault> {
        if catalog::official::owns(&current) {
            let entries = catalog
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("official plugin catalog is unavailable")
                })?
                .as_ref()
                .map_err(Clone::clone)?;
            let entry = entries
                .iter()
                .find(|entry| entry.id == current.summary.name)
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "official plugin is unavailable"))?;
            let available = newer(entry.version.as_deref(), current.summary.version.as_deref())?;
            return Ok(Report {
                available: available.then(|| Candidate::Official(Box::new(entry.clone()))),
                current: current.summary,
                manual: false,
            });
        }
        let name = current.summary.name.clone();
        let (mut candidate, origin) = match &current.origin {
            Some(Origin::Worktree { worktree, path }) => {
                let _permit = self
                    .reads
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Fault::new(ErrorCode::Busy, "plugin inspection is busy"))?;
                let origin = Origin::Worktree {
                    worktree: *worktree,
                    path: path.clone(),
                };
                let path = path.clone();
                let root = root.ok_or_else(|| {
                    sailry_protocol::Fault::invalid("plugin source worktree is unavailable")
                })?;
                let host = self.clone();
                let candidate =
                    tokio::task::spawn_blocking(move || host.inspect(&root, &path, &name, stop))
                        .await
                        .map_err(|_| github::unavailable())??;
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
                        .expect("inspected skill provenance");
                    let origin = Origin::Online {
                        source: provenance.source.clone(),
                        path: provenance.path.clone(),
                    };
                    (candidate, origin)
                } else {
                    let selected = self.inspect_source(&source, stop).await?;
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
        Ok(Report {
            available: (candidate.summary.digest != current.summary.digest)
                .then_some(Candidate::Package(Box::new(candidate))),
            current: current.summary,
            manual: false,
        })
    }
}

fn newer(candidate: Option<&str>, current: Option<&str>) -> Result<bool, Fault> {
    let parse = |version: Option<&str>| {
        version
            .and_then(|version| semver::Version::parse(version).ok())
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid("official plugin version is unavailable")
            })
    };
    Ok(parse(candidate)?.cmp_precedence(&parse(current)?).is_gt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions() {
        for (candidate, current, expected) in [
            ("1.10.0", "1.9.0", true),
            ("1.0.0", "1.0.0", false),
            ("1.0.0", "2.0.0", false),
            ("1.0.0", "1.0.0-rc.1", true),
            ("1.0.0-rc.1", "1.0.0", false),
            ("1.0.0+other", "1.0.0+build", false),
        ] {
            assert_eq!(newer(Some(candidate), Some(current)).unwrap(), expected);
        }
        for (candidate, current) in [
            (None, Some("1.0.0")),
            (Some("1.0.0"), None),
            (Some("invalid"), Some("1.0.0")),
        ] {
            assert!(newer(candidate, current).is_err());
        }
    }
}
