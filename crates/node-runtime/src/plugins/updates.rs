//! Inspect sources without installing packages or changing their configuration.
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    Origin,
    catalog::Entry,
    skills::Source,
    updates::{Candidate, Report},
};

pub(crate) struct Catalog {
    entries: Vec<Entry>,
    repository: tokio::sync::OnceCell<Result<Repository, Fault>>,
}

struct Repository {
    _directory: tempfile::TempDir,
    root: PathBuf,
    links: Vec<String>,
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
                    .map(|entries| Catalog {
                        entries,
                        repository: tokio::sync::OnceCell::new(),
                    }),
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
        catalog: Option<&Result<Catalog, Fault>>,
    ) -> Result<Report, Fault> {
        if catalog::official::owns(&current) {
            let bundled = self.bundled_update(&current, stop.clone()).await?;
            let entry = catalog
                .ok_or_else(|| Fault::invalid("official plugin catalog is unavailable"))
                .and_then(|catalog| catalog.as_ref().map_err(Clone::clone))
                .and_then(|catalog| {
                    catalog
                        .entries
                        .iter()
                        .find(|entry| entry.id == current.summary.name)
                        .map(|entry| (catalog, entry))
                        .ok_or_else(|| {
                            Fault::new(ErrorCode::NotFound, "official plugin is unavailable")
                        })
                });
            let available = match entry {
                Err(error)
                    if !catalog::official::unavailable(&error)
                        && !(error.code == ErrorCode::InvalidRequest && bundled.is_some()) =>
                {
                    return Err(error);
                }
                Ok((catalog, entry)) => {
                    match self.official_update(&current, catalog, entry, stop).await {
                        Ok(available) => available.or_else(|| {
                            bundled.map(|package| Candidate::Package(Box::new(package)))
                        }),
                        Err(error)
                            if catalog::official::unavailable(&error)
                                || error.code == ErrorCode::InvalidRequest && bundled.is_some() =>
                        {
                            if let Some(bundled) = bundled {
                                Some(Candidate::Package(Box::new(bundled)))
                            } else {
                                return Err(error);
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => {
                    if let Some(bundled) = bundled {
                        Some(Candidate::Package(Box::new(bundled)))
                    } else {
                        return Err(error);
                    }
                }
            };
            return Ok(Report {
                available,
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

    async fn official_update(
        &self,
        current: &Info,
        catalog: &Catalog,
        entry: &Entry,
        stop: CancellationToken,
    ) -> Result<Option<Candidate>, Fault> {
        version(entry.version.as_deref())?;
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "plugin inspection is busy"))?;
        let repository = catalog
            .repository
            .get_or_init(|| async { self.update_source(entry, stop.clone()).await })
            .await
            .as_ref()
            .map_err(Clone::clone)?;
        let root = repository.root.clone();
        let links = repository.links.clone();
        let path = entry.id.clone();
        let selected =
            tokio::task::spawn_blocking(move || sources::inspect_root(&root, &path, &links, stop))
                .await
                .map_err(|_| github::unavailable())??;
        if selected.summary.name != current.summary.name {
            return Err(Fault::invalid("plugin update has a different identity"));
        }
        version(selected.summary.version.as_deref())?;
        Ok((selected.summary.digest != current.summary.digest)
            .then(|| Candidate::Official(Box::new(entry.clone()))))
    }

    async fn update_source(
        &self,
        entry: &Entry,
        stop: CancellationToken,
    ) -> Result<Repository, Fault> {
        let source = repository::source::Selection::parse(&Source {
            repository: entry
                .repository
                .clone()
                .ok_or_else(|| Fault::invalid("official plugin repository is unavailable"))?,
            git_ref: Some("main".into()),
            path: None,
        })?;
        let (archive, _) = self.source_archive(&source, None, stop.clone()).await?;
        tokio::task::spawn_blocking(move || {
            let (directory, root, links) = archive::unpack_source(archive, &stop, 10_000)?;
            Ok(Repository {
                _directory: directory,
                root,
                links,
            })
        })
        .await
        .map_err(|_| github::unavailable())?
    }

    async fn bundled_update(
        &self,
        current: &Info,
        stop: CancellationToken,
    ) -> Result<Option<Info>, Fault> {
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
        if bundled.summary.digest == current.summary.digest {
            return Ok(None);
        }
        bundled.origin = Some(Origin::Bundled);
        Ok(Some(bundled))
    }
}

fn version(value: Option<&str>) -> Result<semver::Version, Fault> {
    value
        .and_then(|value| semver::Version::parse(value).ok())
        .ok_or_else(|| sailry_protocol::Fault::invalid("official plugin version is unavailable"))
}
