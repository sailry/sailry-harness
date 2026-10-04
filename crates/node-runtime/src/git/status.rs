//! Bounded candidate scanning and classification adapted from sailry-code
//! 67ae9fa0 sailry-git/src/action_read/status.rs (Apache-2.0).
use super::{Control, git_error, io_error, open, path, valid_path};
use git2::{ObjectType, Oid, Repository, Status};
use sailry_protocol::*;
use std::{collections::BTreeSet, path::Path};

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
        Err(error) => return Err(git_error(error)),
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
    scan_index(&repository, &mut scan, control)?;
    scan_workdir(root, &mut scan, control)?;
    scan_head(&repository, &mut scan, control)?;
    let mut entries = Vec::new();
    for value in &scan.paths {
        control.check()?;
        if super::check_entry(root, value).is_err() {
            scan.omitted_paths += 1;
            continue;
        }
        let status = match repository.status_file(Path::new(value)) {
            Ok(status) => status,
            Err(error) if error.code() == git2::ErrorCode::NotFound => {
                scan.omitted_paths += 1;
                continue;
            }
            Err(error) => return Err(git_error(error)),
        };
        if let Some(entry) = classify(value.clone(), status) {
            if entries.len() == MAX_GIT_ENTRIES {
                scan.truncated = true;
                scan.omitted_paths += 1;
            } else {
                entries.push(entry);
            }
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
fn scan_workdir(root: &Path, scan: &mut Candidates, control: &Control) -> Result<(), Fault> {
    let mut pending = vec![String::new()];
    while let Some(prefix) = pending.pop() {
        control.check()?;
        if !scan.has_budget() {
            scan.truncated = true;
            break;
        }
        let dir = path::descend(path::root(root)?, &path::components(&prefix, true)?)?;
        for child in dir.entries().map_err(io_error)? {
            control.check()?;
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
            if kind.is_dir() {
                scan.scanned_entries += 1;
                if valid_path(&value).is_ok() {
                    pending.push(value);
                } else {
                    scan.omitted_paths += 1;
                }
            } else {
                scan.observe_bytes(value.as_bytes());
            }
        }
    }
    Ok(())
}

fn scan_index(
    repository: &Repository,
    scan: &mut Candidates,
    control: &Control,
) -> Result<(), Fault> {
    let index = repository.index().map_err(git_error)?;
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
        Err(error) => return Err(git_error(error)),
    };
    let tree = head.peel_to_tree().map_err(git_error)?;
    let mut pending = vec![(tree.id(), Vec::<u8>::new())];
    while let Some((tree_id, prefix)) = pending.pop() {
        control.check()?;
        if !scan.has_budget() {
            scan.truncated = true;
            break;
        }
        let tree = repository.find_tree(tree_id).map_err(git_error)?;
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
    if entries.is_empty() {
        return Ok(());
    }
    let head = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_tree().ok());
    let index = repository.index().map_err(git_error)?;
    for scope in [
        GitDiffScope::All,
        GitDiffScope::Staged,
        GitDiffScope::Unstaged,
    ] {
        let mut options = git2::DiffOptions::new();
        options
            .disable_pathspec_match(true)
            .ignore_submodules(true)
            .max_size(1024 * 1024);
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
        .map_err(git_error)?;
        for position in 0..diff.deltas().len() {
            control.check()?;
            let Some(patch) = git2::Patch::from_diff(&diff, position).map_err(git_error)? else {
                continue;
            };
            let delta = patch.delta();
            let path = delta.new_file().path().or_else(|| delta.old_file().path());
            if let Some(entry) = entries
                .iter_mut()
                .find(|entry| path == Some(Path::new(&entry.path)))
            {
                let (_, additions, deletions) = patch.line_stats().map_err(git_error)?;
                let stats = GitLineStats {
                    additions,
                    deletions,
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
