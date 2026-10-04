//! Repository inspection and installation share acquisition and package validation.
use super::repository::source;
use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{
    catalog::Selection,
    skills::{Resolved, Source},
};

impl Host {
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
        let (archive, source) = self.source_archive(&selected, None, stop.clone()).await?;
        tokio::task::spawn_blocking(move || {
            let (_temporary, root) = selected_root(archive, &selected.path, &stop)?;
            let directory = crate::files::path::root(&root)?;
            let bytes = package::read(&directory, "plugin.json", 64 * 1024)?
                .ok_or_else(|| invalid("plugin.json is required at the selected directory"))?;
            let mut tree = package::Tree::default();
            tree.stop = stop;
            tree.walk(&directory, None, "", 0)?;
            let info = describe(
                &directory,
                manifest::parse(&bytes)?,
                tree.digest(),
                tree.issues,
            )?;
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
    ) -> Result<Info, Fault> {
        let selected = source::Selection::resolved(source, path)?;
        let stop = CancellationToken::new();
        let (archive, _) = self
            .source_archive(&selected, Some(&source.commit), stop.clone())
            .await?;
        let host = self.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || {
            let (_temporary, root) = selected_root(archive, &selected.path, &stop)?;
            host.install(&root, "", &name)
        })
        .await
        .map_err(|_| github::unavailable())?
    }
}

fn selected_root(
    file: std::fs::File,
    path: &str,
    stop: &CancellationToken,
) -> Result<(tempfile::TempDir, PathBuf), Fault> {
    let (temporary, root, links) = archive::unpack_source(file, stop, 10_000)?;
    if links.iter().any(|link| {
        path.is_empty()
            || path == link
            || path.starts_with(&format!("{link}/"))
            || link.starts_with(&format!("{path}/"))
    }) {
        return Err(invalid(
            "selected plugin contains unsupported symbolic links",
        ));
    }
    let directory = crate::files::path::descend(
        crate::files::path::root(&root)?,
        &crate::files::path::components(path, true)?,
    )?;
    drop(directory);
    Ok((temporary, root.join(path)))
}
