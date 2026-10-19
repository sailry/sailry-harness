//! Repository inspection and installation share acquisition and package validation.
mod catalog;
use super::repository::source;
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    catalog::Selection,
    skills::{Resolved, Source},
};

impl Host {
    pub(super) async fn inspect_newer_source(
        &self,
        source: &Source,
        current: &sailry_protocol::plugin::Summary,
        stop: CancellationToken,
    ) -> Result<Option<Selection>, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "plugin inspection is busy"))?;
        let selected = source::Selection::parse(source)?;
        let (bytes, resolved) = self.source_manifest(&selected, stop.clone()).await?;
        let manifest = manifest::parse(&bytes)?;
        if manifest.name != current.name {
            return Err(Fault::invalid("plugin update has a different identity"));
        }
        if !updates::version(manifest.version.as_deref())?
            .cmp_precedence(&updates::version(current.version.as_deref())?)
            .is_gt()
        {
            return Ok(None);
        }
        let expected = manifest.version;
        let (archive, resolved) = self
            .source_archive(&selected, Some(&resolved.commit), stop.clone(), None)
            .await?;
        tokio::task::spawn_blocking(move || {
            let (_temporary, root, links) = archive::unpack_source(archive, &stop, 10_000)?;
            let info = inspect_root(&root, &selected.path, &links, stop)?;
            if info.summary.version != expected {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "plugin version changed; check updates again",
                ));
            }
            Ok(Some(Selection {
                source: resolved,
                path: selected.path,
                info,
            }))
        })
        .await
        .map_err(|_| github::unavailable())?
    }

    pub(crate) async fn inspect_source(
        &self,
        source: &Source,
        stop: CancellationToken,
    ) -> Result<Selection, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "plugin inspection is busy"))?;
        let selected = source::Selection::parse(source)?;
        let (archive, source) = self
            .source_archive(&selected, None, stop.clone(), None)
            .await?;
        tokio::task::spawn_blocking(move || {
            let (_temporary, root, links) = archive::unpack_source(archive, &stop, 10_000)?;
            let info = inspect_root(&root, &selected.path, &links, stop)?;
            Ok(Selection {
                source,
                path: selected.path,
                info,
            })
        })
        .await
        .map_err(|_| github::unavailable())?
    }

    pub(crate) async fn install_source(
        &self,
        source: &Resolved,
        path: &str,
        name: &str,
        stop: CancellationToken,
        reporter: Option<crate::installation::Reporter>,
    ) -> Result<Info, Fault> {
        let selected = source::Selection::resolved(source, path)?;
        let (archive, _) = self
            .source_archive(
                &selected,
                Some(&source.commit),
                stop.clone(),
                reporter.clone(),
            )
            .await?;
        let host = self.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Some(reporter) = reporter {
                reporter(sailry_protocol::installation::Phase::Inspecting, None);
            }
            let (_temporary, root) = selected_root(archive, &selected.path, &stop)?;
            host.install(&root, "", &name)
        })
        .await
        .map_err(|_| github::unavailable())?
    }
}

pub(super) fn inspect_root(
    root: &Path,
    path: &str,
    links: &[String],
    stop: CancellationToken,
) -> Result<Info, Fault> {
    let directory = crate::files::path::root(&package_root(root, path, links)?)?;
    let bytes = package::read(&directory, "plugin.json", 64 * 1024)?.ok_or_else(|| {
        sailry_protocol::Fault::invalid("plugin.json is required at the selected directory")
    })?;
    let mut tree = package::Tree::default();
    tree.stop = stop;
    tree.walk(&directory, None, "", 0)?;
    describe(
        &directory,
        manifest::parse(&bytes)?,
        tree.digest(),
        tree.issues,
    )
}

fn selected_root(
    file: std::fs::File,
    path: &str,
    stop: &CancellationToken,
) -> Result<(tempfile::TempDir, PathBuf), Fault> {
    let (temporary, root, links) = archive::unpack_source(file, stop, 10_000)?;
    Ok((temporary, package_root(&root, path, &links)?))
}

pub(in crate::plugins) fn package_root(
    root: &Path,
    path: &str,
    links: &[String],
) -> Result<PathBuf, Fault> {
    if links.iter().any(|link| {
        path.is_empty()
            || path == link
            || path.starts_with(&format!("{link}/"))
            || link.starts_with(&format!("{path}/"))
    }) {
        return Err(sailry_protocol::Fault::invalid(
            "selected plugin contains unsupported symbolic links",
        ));
    }
    let directory = crate::files::path::descend(
        crate::files::path::root(root)?,
        &crate::files::path::components(path, true)?,
    )?;
    drop(directory);
    Ok(root.join(path))
}
