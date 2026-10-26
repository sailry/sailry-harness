//! Bounded candidate scanning and classification adapted from sailry-code
//! 67ae9fa0 sailry-git/src/action_read/status.rs (Apache-2.0).
use super::{Control, EntryParents, changes::Stamp, fault, head, io_error, open, path, valid_path};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use git2::{ObjectType, Oid, Repository, Status, StatusOptions};
use sailry_protocol::*;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Metadata {
    gitdir: (PathBuf, path::identity::Identity),
    common: (PathBuf, path::identity::Identity),
    head: GitHead,
    head_stamp: Option<Stamp>,
    index: Option<Stamp>,
    packed: Option<Stamp>,
    reference: Option<(String, Option<Stamp>)>,
}

pub(super) fn metadata(root: &Path) -> Result<Option<Metadata>, Fault> {
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let gitdir = path::root(repository.path())?;
    let common = path::root(repository.commondir())?;
    let reference = match repository.head() {
        Ok(reference) => Some(reference.name().map_err(fault)?)
            .filter(|name| *name != "HEAD")
            .map(str::to_owned),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => repository
            .find_reference("HEAD")
            .map_err(fault)?
            .symbolic_target()
            .map_err(fault)?
            .map(str::to_owned),
        Err(error) => return Err(fault(error)),
    }
    .map(|name| -> Result<_, Fault> {
        valid_path(&name)?;
        let value = stamp(&common, &name)?;
        Ok((name, value))
    })
    .transpose()?;
    Ok(Some(Metadata {
        gitdir: (
            repository.path().to_owned(),
            path::identity::directory(&gitdir)?,
        ),
        common: (
            repository.commondir().to_owned(),
            path::identity::directory(&common)?,
        ),
        head: head::project(&repository)?,
        head_stamp: stamp(&gitdir, "HEAD")?,
        index: stamp(&gitdir, "index")?,
        packed: stamp(&common, "packed-refs")?,
        reference,
    }))
}

fn stamp(root: &Dir, name: &str) -> Result<Option<Stamp>, Fault> {
    match root.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
            Ok(Some(Stamp::of(&metadata)))
        }
        Ok(_) => Err(Fault::invalid("expected a regular Git resource")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

pub(super) fn inspect(root: &Path, control: &Control) -> Result<GitStatus, Fault> {
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
            return Ok(GitStatus {
                kind: RepositoryKind::Directory,
                branch: None,
                head: None,
                index_revision: None,
                entries: vec![],
                truncated: false,
                omitted_paths: 0,
            });
        }
        Err(error) => return Err(error),
    };
    let head = match repository.head() {
        Ok(head) => Some(head),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => return Err(fault(error)),
    };
    let branch = head
        .as_ref()
        .filter(|head| head.is_branch())
        .and_then(|head| head.shorthand().ok())
        .map(str::to_owned)
        .or_else(|| {
            if head.is_none() {
                repository
                    .find_reference("HEAD")
                    .ok()
                    .and_then(|reference| {
                        reference
                            .symbolic_target()
                            .ok()??
                            .strip_prefix("refs/heads/")
                            .map(str::to_owned)
                    })
            } else {
                None
            }
        });
    let head_id = head
        .as_ref()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());
    let mut scan = Candidates {
        paths: BTreeSet::new(),
        scanned_entries: 0,
        omitted_paths: 0,
        budget: 20_000,
        truncated: false,
    };
    let retained = path::root(root)?;
    scan_index(&repository, &mut scan, control)?;
    scan_head(&repository, &mut scan, control)?;
    scan_workdir(&retained, &repository, &mut scan, control)?;
    // A single status pass avoids rebuilding HEAD/index comparisons for every file.
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .include_unmodified(false)
        .disable_pathspec_match(true)
        .renames_head_to_index(false)
        .renames_index_to_workdir(false);
    for value in &scan.paths {
        control.check()?;
        options.pathspec(value);
    }
    // An empty pathspec means the entire worktree, not an empty selection.
    if scan.paths.is_empty() {
        options.pathspec(".git");
    }
    let statuses = repository.statuses(Some(&mut options)).map_err(fault)?;
    let mut entries = Vec::new();
    let mut parents = EntryParents::new(retained);
    for candidate in statuses.iter() {
        control.check()?;
        let Some(value) = candidate
            .path()
            .ok()
            .filter(|value| scan.paths.contains(*value))
        else {
            continue;
        };
        let Some(entry) = classify(value.to_owned(), candidate.status()) else {
            continue;
        };
        if entries.len() == MAX_GIT_ENTRIES {
            scan.truncated = true;
            scan.omitted_paths += 1;
        } else if parents.check(value, false).is_err() {
            scan.omitted_paths += 1;
        } else {
            entries.push(entry);
        }
    }
    statistics(&repository, &mut entries, control)?;
    Ok(GitStatus {
        kind: if head.is_some() {
            RepositoryKind::Ready
        } else {
            RepositoryKind::Unborn
        },
        branch,
        head: head_id,
        index_revision: Some(super::index::revision(&repository)?),
        entries,
        truncated: scan.truncated,
        omitted_paths: scan.omitted_paths,
    })
}

struct Candidates {
    paths: BTreeSet<String>,
    scanned_entries: usize,
    omitted_paths: usize,
    budget: usize,
    truncated: bool,
}
impl Candidates {
    fn has_budget(&self) -> bool {
        self.scanned_entries < self.budget
    }
    fn observe_bytes(&mut self, value: &[u8]) {
        self.scanned_entries += 1;
        match std::str::from_utf8(value)
            .ok()
            .filter(|value| valid_path(value).is_ok())
        {
            Some(value) => {
                self.paths.insert(value.to_owned());
            }
            None => self.omitted_paths += 1,
        }
    }
}

// Retain directory capabilities while enumerating; never descend through symlinks.
fn scan_workdir(
    root: &Dir,
    repository: &Repository,
    scan: &mut Candidates,
    control: &Control,
) -> Result<(), Fault> {
    let mut pending = vec![(
        String::new(),
        root.try_clone().map_err(io_error)?,
        root.entries().map_err(io_error)?,
    )];
    while let Some((prefix, dir, children)) = pending.last_mut() {
        control.check()?;
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if !scan.has_budget() {
            scan.truncated = true;
            break;
        }
        let child = child.map_err(io_error)?;
        let name = child.file_name();
        let Some(name) = name.to_str() else {
            scan.scanned_entries += 1;
            scan.omitted_paths += 1;
            continue;
        };
        if name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let value = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = child.file_type().map_err(io_error)?;
        // Tracked paths are retained from the index and HEAD even under an ignored directory.
        let ignore_path = if kind.is_dir() {
            format!("{value}/")
        } else {
            value.clone()
        };
        if repository
            .status_should_ignore(Path::new(&ignore_path))
            .map_err(fault)?
        {
            continue;
        }
        if kind.is_dir() {
            scan.scanned_entries += 1;
            if valid_path(&value).is_err() {
                scan.omitted_paths += 1;
                continue;
            }
            let child = dir.open_dir_nofollow(name).map_err(io_error)?;
            #[cfg(windows)]
            path::identity::directory(&child)?;
            let children = child.entries().map_err(io_error)?;
            pending.push((value, child, children));
        } else if kind.is_file() || kind.is_symlink() {
            scan.observe_bytes(value.as_bytes());
        } else {
            scan.scanned_entries += 1;
            scan.omitted_paths += 1;
            scan.paths.remove(&value);
        }
    }
    Ok(())
}

fn scan_index(
    repository: &Repository,
    scan: &mut Candidates,
    control: &Control,
) -> Result<(), Fault> {
    let index = repository.index().map_err(fault)?;
    for entry in index.iter() {
        control.check()?;
        if !scan.has_budget() {
            scan.truncated = true;
            break;
        }
        scan.observe_bytes(&entry.path);
    }
    Ok(())
}

fn scan_head(
    repository: &Repository,
    scan: &mut Candidates,
    control: &Control,
) -> Result<(), Fault> {
    let head = match repository.head() {
        Ok(head) => head,
        Err(error)
            if matches!(
                error.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(fault(error)),
    };
    let tree = head.peel_to_tree().map_err(fault)?;
    let mut pending = vec![(tree.id(), Vec::<u8>::new())];
    while let Some((tree_id, prefix)) = pending.pop() {
        control.check()?;
        if !scan.has_budget() {
            scan.truncated = true;
            break;
        }
        let tree = repository.find_tree(tree_id).map_err(fault)?;
        let mut child_trees: Vec<(Oid, Vec<u8>)> = Vec::new();
        for entry in &tree {
            control.check()?;
            if !scan.has_budget() {
                scan.truncated = true;
                break;
            }
            let path = joined_bytes(&prefix, entry.name_bytes());
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    scan.scanned_entries += 1;
                    child_trees.push((entry.id(), path));
                }
                Some(ObjectType::Blob) | Some(ObjectType::Commit) => scan.observe_bytes(&path),
                _ => scan.scanned_entries += 1,
            }
        }
        child_trees.reverse();
        pending.extend(child_trees);
    }
    Ok(())
}

fn joined_bytes(prefix: &[u8], name: &[u8]) -> Vec<u8> {
    let mut path = Vec::with_capacity(prefix.len() + usize::from(!prefix.is_empty()) + name.len());
    path.extend_from_slice(prefix);
    if !prefix.is_empty() {
        path.push(b'/');
    }
    path.extend_from_slice(name);
    path
}

fn classify(path: String, status: Status) -> Option<GitEntry> {
    let conflicted = status.is_conflicted();
    let untracked = status.is_wt_new();
    let staged = if status.is_index_deleted() {
        Some(GitChangeKind::Deleted)
    } else if status.is_index_typechange() {
        Some(GitChangeKind::TypeChanged)
    } else if status.is_index_renamed() {
        Some(GitChangeKind::Renamed)
    } else if status.is_index_new() {
        Some(GitChangeKind::Added)
    } else if status.is_index_modified() {
        Some(GitChangeKind::Modified)
    } else {
        None
    };
    let unstaged = if status.is_wt_deleted() {
        Some(GitChangeKind::Deleted)
    } else if status.is_wt_typechange() {
        Some(GitChangeKind::TypeChanged)
    } else if status.is_wt_renamed() {
        Some(GitChangeKind::Renamed)
    } else if status.is_wt_modified() {
        Some(GitChangeKind::Modified)
    } else {
        None
    };
    if staged.is_none() && unstaged.is_none() && !untracked && !conflicted {
        return None;
    }
    Some(GitEntry {
        diff: Default::default(),
        staged_diff: Default::default(),
        unstaged_diff: Default::default(),
        path,
        staged,
        unstaged,
        untracked,
        conflicted,
    })
}

fn statistics(
    repository: &Repository,
    entries: &mut [GitEntry],
    control: &Control,
) -> Result<(), Fault> {
    const MAX_BYTES: i64 = 1024 * 1024;
    if entries.is_empty() {
        return Ok(());
    }
    let head = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_tree().ok());
    let index = repository.index().map_err(fault)?;
    for scope in [
        GitDiffScope::All,
        GitDiffScope::Staged,
        GitDiffScope::Unstaged,
    ] {
        let mut options = git2::DiffOptions::new();
        options
            .disable_pathspec_match(true)
            .ignore_submodules(true)
            .max_size(MAX_BYTES);
        for entry in entries.iter() {
            options.pathspec(&entry.path);
        }
        if scope != GitDiffScope::Staged {
            options
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .show_untracked_content(true);
        }
        let diff = match scope {
            GitDiffScope::All => {
                repository.diff_tree_to_workdir_with_index(head.as_ref(), Some(&mut options))
            }
            GitDiffScope::Staged => {
                repository.diff_tree_to_index(head.as_ref(), Some(&index), Some(&mut options))
            }
            GitDiffScope::Unstaged => {
                repository.diff_index_to_workdir(Some(&index), Some(&mut options))
            }
        }
        .map_err(fault)?;
        for position in 0..diff.deltas().len() {
            control.check()?;
            let patch = git2::Patch::from_diff(&diff, position).map_err(fault)?;
            let Some(delta) = diff.get_delta(position) else {
                continue;
            };
            let path = delta.new_file().path().or_else(|| delta.old_file().path());
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| path == Some(Path::new(&entry.path)))
            {
                let (_, additions, deletions) = patch
                    .as_ref()
                    .map(|patch| patch.line_stats())
                    .transpose()
                    .map_err(fault)?
                    .unwrap_or_default();
                let stats = GitLineStats {
                    additions,
                    deletions,
                    truncated: delta.old_file().size() > MAX_BYTES as u64
                        || delta.new_file().size() > MAX_BYTES as u64,
                };
                match scope {
                    GitDiffScope::All => entry.diff = stats,
                    GitDiffScope::Staged => entry.staged_diff = stats,
                    GitDiffScope::Unstaged => entry.unstaged_diff = stats,
                }
            }
        }
    }
    Ok(())
}
