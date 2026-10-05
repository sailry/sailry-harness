//! Literal-path diff and bounded patch callbacks adapted from sailry-code
//! 67ae9fa0 sailry-git/src/action_read/diff.rs (Apache-2.0).
use std::path::Path;

use git2::{DiffFormat, DiffOptions};
use sailry_protocol::*;

use super::{Control, check_entry, fault, open, valid_path};

pub(super) fn read(
    root: &Path,
    path: String,
    scope: GitDiffScope,
    control: &Control,
) -> Result<GitDiff, Fault> {
    valid_path(&path)?;
    check_entry(root, &path)?;
    let repository = open(root)?;
    let tree = match repository.head() {
        Ok(head) => Some(head.peel_to_tree().map_err(fault)?),
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => None,
        Err(error) => return Err(fault(error)),
    };
    let index = repository.index().map_err(fault)?;
    let mut options = DiffOptions::new();
    options
        .pathspec(&path)
        .disable_pathspec_match(true)
        .ignore_submodules(true)
        .max_size(1024 * 1024);
    if scope != GitDiffScope::Staged {
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true);
    }
    control.check()?;
    let diff = match scope {
        GitDiffScope::All => {
            repository.diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut options))
        }
        GitDiffScope::Staged => {
            repository.diff_tree_to_index(tree.as_ref(), Some(&index), Some(&mut options))
        }
        GitDiffScope::Unstaged => {
            repository.diff_index_to_workdir(Some(&index), Some(&mut options))
        }
    }
    .map_err(fault)?;
    let result = print(&diff, path, scope, control)?;
    check_entry(root, &result.path)?;
    Ok(result)
}

pub(super) fn print(
    diff: &git2::Diff<'_>,
    path: String,
    scope: GitDiffScope,
    control: &Control,
) -> Result<GitDiff, Fault> {
    let mut files = render(diff, Some(path.clone()), scope, control)?;
    Ok(files.pop().unwrap_or_else(|| empty(path, scope)))
}

pub(super) fn files(diff: &git2::Diff<'_>, control: &Control) -> Result<Vec<GitDiff>, Fault> {
    render(diff, None, GitDiffScope::All, control)
}

fn empty(path: String, scope: GitDiffScope) -> GitDiff {
    GitDiff {
        path,
        scope,
        text: String::new(),
        additions: 0,
        deletions: 0,
        binary: false,
        truncated: false,
    }
}

fn render(
    diff: &git2::Diff<'_>,
    path: Option<String>,
    scope: GitDiffScope,
    control: &Control,
) -> Result<Vec<GitDiff>, Fault> {
    let mut files: Vec<GitDiff> = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    let mut cancelled = None;
    let printed = diff.print(DiffFormat::Patch, |delta, _, line| {
        if let Err(error) = control.check() {
            cancelled = Some(error);
            return false;
        }
        let path = path.clone().unwrap_or_else(|| {
            String::from_utf8_lossy(
                delta
                    .new_file()
                    .path_bytes()
                    .or_else(|| delta.old_file().path_bytes())
                    .unwrap_or_default(),
            )
            .into_owned()
        });
        if files.last().is_none_or(|file| file.path != path) {
            files.push(empty(path, scope));
        }
        let result = files.last_mut().unwrap();
        result.binary |=
            delta.old_file().is_binary() || delta.new_file().is_binary() || line.origin() == 'B';
        let prefix = matches!(line.origin(), '+' | '-' | ' ');
        let content = String::from_utf8_lossy(line.content());
        let required = content.len() + usize::from(prefix);
        if bytes + required > MAX_DIFF_BYTES {
            result.truncated = true;
            truncated = true;
            return false;
        }
        bytes += required;
        if prefix {
            result.text.push(line.origin());
        }
        result.text.push_str(&content);
        result.additions += usize::from(line.origin() == '+');
        result.deletions += usize::from(line.origin() == '-');
        true
    });
    if let Some(error) = cancelled {
        return Err(error);
    }
    if !truncated {
        printed.map_err(fault)?;
    }
    control.check()?;
    Ok(files)
}
