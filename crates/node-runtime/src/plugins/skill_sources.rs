//! Standalone skills reuse the shared repository acquisition boundary.
use super::github::{Github, cancelled, unavailable};
use super::repository::source;
use super::{Host, io_error};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    plugin::{
        Info,
        skills::{Candidate, Discovery, GitProvenance, Provenance, Resolved, Source},
    },
};
use std::fs::File;
mod directory;
mod imports;
pub(super) mod package;
impl Host {
    pub(crate) fn with_github(mut self, github: Github) -> Self {
        self.github = github;
        self
    }

    pub(crate) async fn discover_skills(
        &self,
        source: &Source,
        stop: CancellationToken,
    ) -> Result<Discovery, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "skill discovery is busy"))?;
        let (source, selected_skill) = directory::resolve(source)?;
        let selected = source::Selection::parse(&source)?;
        let (file, source) = self
            .source_archive(&selected, None, stop.clone(), None)
            .await?;
        let mut skills =
            tokio::task::spawn_blocking(move || package::discover(file, &selected, &stop))
                .await
                .map_err(|_| unavailable())??;
        if let Some(name) = selected_skill {
            directory::select(&mut skills, &name)?;
        }
        Ok(Discovery { source, skills })
    }

    pub(crate) async fn install_skill(
        &self,
        source: &Resolved,
        path: &str,
        name: &str,
        stop: CancellationToken,
        reporter: Option<crate::installation::Reporter>,
    ) -> Result<Info, Fault> {
        let selected = source::Selection::resolved(source, path)?;
        if name != source::name(&source.repository, path) {
            return Err(sailry_protocol::Fault::invalid(
                "skill identity does not match its source",
            ));
        }
        let (file, _) = self
            .source_archive(
                &selected,
                Some(&source.commit),
                stop.clone(),
                reporter.clone(),
            )
            .await?;
        let host = self.clone();
        let provenance = Provenance::Git(GitProvenance {
            source: source.clone(),
            path: path.into(),
        });
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || {
            if let Some(reporter) = reporter {
                reporter(sailry_protocol::installation::Phase::Inspecting, None);
            }
            package::install(&host, file, &name, provenance, &stop)
        })
        .await
        .map_err(|_| unavailable())?
    }

    pub(crate) async fn inspect_skill(
        &self,
        source: &Source,
        name: &str,
        stop: CancellationToken,
    ) -> Result<Info, Fault> {
        let _permit = self
            .reads
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "skill inspection is busy"))?;
        let selected = source::Selection::parse(source)?;
        if name != source::name(&selected.repository, &selected.path) {
            return Err(sailry_protocol::Fault::invalid(
                "skill identity does not match its source",
            ));
        }
        let (file, resolved) = self
            .source_archive(&selected, None, stop.clone(), None)
            .await?;
        let host = self.clone();
        let name = name.to_owned();
        let provenance = Provenance::Git(GitProvenance {
            source: resolved,
            path: selected.path,
        });
        tokio::task::spawn_blocking(move || package::inspect(&host, file, &name, provenance, &stop))
            .await
            .map_err(|_| unavailable())?
    }
}
