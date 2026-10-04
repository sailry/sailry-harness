//! Skills and plugins share Git acquisition, without changing their package validators.
use super::github::cancelled;
use super::{Host, invalid, io_error};
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
    pub(super) async fn source_archive(
        &self,
        selected: &source::Selection,
        commit: Option<&str>,
        stop: CancellationToken,
    ) -> Result<(File, Resolved), Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if self.github.endpoint.is_some() {
            // Existing HTTP fixtures replace acquisition explicitly; production never falls back.
            let selected = super::github::source::Selection::parse(&Source {
                repository: selected.repository.clone(),
                git_ref: Some(selected.git_ref.clone()),
                path: Some(selected.path.clone()),
            })?;
            let resolved = if let Some(commit) = commit {
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
        tokio::task::spawn_blocking(move || git::acquire(&selected, commit.as_deref(), &stop))
            .await
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git source worker is unavailable"))?
    }
}
