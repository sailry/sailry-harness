//! Export a pinned Git tree, without checkout, hooks, submodules or package execution.
use super::*;
use git2::{ObjectType, Repository};
use sailry_protocol::plugin::MAX_PACKAGE_BYTES;
use std::{
    io::Write,
    time::{Duration, Instant},
};

// Git transfers include unrelated repository objects; package limits apply to the export below.
const MAX_TRANSFER_BYTES: u64 = 512 * 1024 * 1024;

struct Fetched {
    repository: Repository,
    _directory: tempfile::TempDir,
    source: Resolved,
}

fn fetch(
    selected: &source::Selection,
    commit: Option<&str>,
    stop: &CancellationToken,
    reporter: Option<crate::installation::Reporter>,
) -> Result<Fetched, Fault> {
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    let temporary = tempfile::tempdir().map_err(io_error)?;
    let repository = Repository::init_bare(temporary.path()).map_err(failure)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut callbacks = crate::git::credentials::callbacks();
    let oversized = std::cell::Cell::new(false);
    callbacks.transfer_progress(|progress| {
        if let Some(reporter) = &reporter {
            let completed = progress.received_objects() as u64;
            let total = progress.total_objects() as u64;
            // libgit2 reports object totals, not a download byte total.
            if total > 0 && completed <= total {
                reporter(
                    sailry_protocol::installation::Phase::Downloading,
                    Some(sailry_protocol::installation::Progress::Counts {
                        completed,
                        total,
                        bytes_per_second: None,
                    }),
                );
            }
        }
        oversized.set(progress.received_bytes() as u64 > MAX_TRANSFER_BYTES);
        !oversized.get() && !stop.is_cancelled() && Instant::now() < deadline
    });
    callbacks.sideband_progress(|_| !stop.is_cancelled() && Instant::now() < deadline);
    let mut fetch = git2::FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    fetch.download_tags(git2::AutotagOption::None);
    // libgit2's local transport does not support shallow fetches.
    if !selected.repository.starts_with("file:") {
        fetch.depth(1);
    }
    let mut remote = repository
        .remote_anonymous(&selected.repository)
        .map_err(failure)?;
    // A durable install fetches only its captured commit, never re-resolves a moving branch.
    let revision = commit.unwrap_or(&selected.git_ref);
    let result = remote.fetch(
        &[format!("{revision}:refs/sailry/source")],
        Some(&mut fetch),
        None,
    );
    drop(remote);
    drop(fetch);
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    if oversized.get() {
        return Err(sailry_protocol::Fault::invalid(
            "Git transfer exceeds its size limit",
        ));
    }
    if Instant::now() >= deadline {
        return Err(Fault::new(ErrorCode::Unavailable, "Git transfer timed out"));
    }
    result.map_err(failure)?;
    if let Some(reporter) = &reporter {
        reporter(sailry_protocol::installation::Phase::Inspecting, None);
    }
    let resolved = {
        let object = if let Some(commit) = commit {
            repository
                .find_object(
                    git2::Oid::from_str(commit).map_err(failure)?,
                    Some(ObjectType::Commit),
                )
                .map_err(failure)?
        } else {
            repository
                .revparse_single("refs/sailry/source")
                .map_err(failure)?
        };
        let commit = object.peel_to_commit().map_err(failure)?;
        Resolved {
            repository: selected.repository.clone(),
            git_ref: selected.git_ref.clone(),
            commit: commit.id().to_string(),
        }
    };
    Ok(Fetched {
        _directory: temporary,
        repository,
        source: resolved,
    })
}

pub(super) fn manifest(
    selected: &source::Selection,
    stop: &CancellationToken,
) -> Result<(Vec<u8>, Resolved), Fault> {
    let fetched = fetch(selected, None, stop, None)?;
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    let commit = fetched
        .repository
        .find_commit(git2::Oid::from_str(&fetched.source.commit).map_err(failure)?)
        .map_err(failure)?;
    let tree = commit.tree().map_err(failure)?;
    let path = std::path::Path::new(&selected.path).join("plugin.json");
    let entry = tree.get_path(&path).map_err(failure)?;
    if entry.kind() != Some(ObjectType::Blob) || !matches!(entry.filemode(), 0o100644 | 0o100755) {
        return Err(Fault::invalid(
            "selected plugin manifest must be a regular file",
        ));
    }
    let blob = fetched.repository.find_blob(entry.id()).map_err(failure)?;
    if blob.size() > 64 * 1024 {
        return Err(Fault::invalid("plugin manifest exceeds its size limit"));
    }
    Ok((blob.content().to_vec(), fetched.source.clone()))
}

pub(super) fn acquire(
    selected: &source::Selection,
    commit: Option<&str>,
    stop: &CancellationToken,
    reporter: Option<crate::installation::Reporter>,
) -> Result<(File, Resolved), Fault> {
    let fetched = fetch(selected, commit, stop, reporter)?;
    let repository = &fetched.repository;
    let mut zip = zip::ZipWriter::new(tempfile::tempfile().map_err(io_error)?);
    let mut state = Export {
        repository,
        zip: &mut zip,
        count: 0,
        bytes: 0,
        stop,
    };
    let commit = repository
        .find_commit(git2::Oid::from_str(&fetched.source.commit).map_err(failure)?)
        .map_err(failure)?;
    let tree = commit.tree().map_err(failure)?;
    let (selected_tree, prefix) = if selected.path.is_empty() {
        (tree.clone(), "source".to_owned())
    } else {
        let entry = tree
            .get_path(std::path::Path::new(&selected.path))
            .map_err(failure)?;
        if entry.kind() != Some(ObjectType::Tree) {
            return Err(sailry_protocol::Fault::invalid(
                "selected source must be a directory",
            ));
        }
        (
            repository.find_tree(entry.id()).map_err(failure)?,
            format!("source/{}", selected.path),
        )
    };
    state.write(&selected_tree, &prefix, 0, None)?;
    if !selected.path.is_empty() {
        state.write(&tree, "source", 0, Some(&LICENSES))?;
    }
    let file = zip
        .finish()
        .map_err(|_| sailry_protocol::Fault::invalid("Git tree could not be exported"))?;
    Ok((file, fetched.source.clone()))
}

struct Export<'a> {
    repository: &'a Repository,
    zip: &'a mut zip::ZipWriter<File>,
    count: usize,
    bytes: u64,
    stop: &'a CancellationToken,
}

impl Export<'_> {
    fn write(
        &mut self,
        tree: &git2::Tree<'_>,
        prefix: &str,
        depth: usize,
        names: Option<&[&str]>,
    ) -> Result<(), Fault> {
        if depth > 32 {
            return Err(sailry_protocol::Fault::invalid(
                "repository nesting is too deep",
            ));
        }
        for entry in tree {
            if self.stop.is_cancelled() {
                return Err(cancelled());
            }
            let name = entry.name().map_err(|_| {
                sailry_protocol::Fault::invalid("repository filenames must be UTF-8")
            })?;
            if names.is_some_and(|names| !names.contains(&name)) {
                continue;
            }
            self.count += 1;
            if self.count > 10_000 {
                return Err(sailry_protocol::Fault::invalid(
                    "Git tree has too many entries",
                ));
            }
            crate::files::path::entry_components(name)?;
            let path = format!("{prefix}/{name}");
            if entry.kind() == Some(ObjectType::Tree) {
                self.write(
                    &self.repository.find_tree(entry.id()).map_err(failure)?,
                    &path,
                    depth + 1,
                    None,
                )?;
                continue;
            }
            if entry.kind() == Some(ObjectType::Commit) {
                // Report gitlinks like omitted links; existing selection validation rejects them.
                self.zip
                    .add_symlink(
                        &path,
                        "unsupported-submodule",
                        zip::write::SimpleFileOptions::default(),
                    )
                    .map_err(|_| {
                        sailry_protocol::Fault::invalid("Git tree could not be exported")
                    })?;
                continue;
            }
            let blob = self.repository.find_blob(entry.id()).map_err(failure)?;
            self.bytes = self
                .bytes
                .checked_add(blob.size() as u64)
                .filter(|bytes| *bytes <= MAX_PACKAGE_BYTES)
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("Git tree exceeds its expanded size limit")
                })?;
            if entry.filemode() == 0o120000 {
                let target = std::str::from_utf8(blob.content()).map_err(|_| {
                    sailry_protocol::Fault::invalid("repository symbolic link must be UTF-8")
                })?;
                self.zip
                    .add_symlink(&path, target, zip::write::SimpleFileOptions::default())
                    .map_err(|_| {
                        sailry_protocol::Fault::invalid("Git tree could not be exported")
                    })?;
            } else {
                let options = zip::write::SimpleFileOptions::default().unix_permissions(
                    if entry.filemode() & 0o111 != 0 {
                        0o755
                    } else {
                        0o644
                    },
                );
                self.zip.start_file(&path, options).map_err(|_| {
                    sailry_protocol::Fault::invalid("Git tree could not be exported")
                })?;
                self.zip.write_all(blob.content()).map_err(io_error)?;
            }
        }
        Ok(())
    }
}

fn failure(error: git2::Error) -> Fault {
    // Git errors may include private URLs or credential-helper output.
    Fault::new(
        match error.code() {
            git2::ErrorCode::Auth => ErrorCode::PermissionDenied,
            git2::ErrorCode::NotFound => ErrorCode::NotFound,
            _ => ErrorCode::Unavailable,
        },
        "Git source could not be read",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_acquisition_never_connects() {
        let selected = source::Selection {
            repository: "https://example.invalid/repo.git".into(),
            git_ref: "HEAD".into(),
            path: String::new(),
        };
        let stop = CancellationToken::new();
        stop.cancel();
        assert_eq!(
            acquire(&selected, None, &stop, None).unwrap_err().code,
            ErrorCode::Cancelled
        );
    }
}
