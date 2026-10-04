//! Bounded HEAD traversal and reference labels adapted from sailry-code
//! 67ae9fa0 sailry-git/src/action_read/log.rs (Apache-2.0).
use std::{collections::HashMap, path::Path};

use git2::{Oid, Repository, Sort};
use sailry_protocol::*;

use super::{Control, diff, git_error, open};

pub(super) fn log(
    root: &Path,
    limit: usize,
    cursor: Option<&GitLogCursor>,
    control: &Control,
) -> Result<GitLog, Fault> {
    if limit == 0 || limit > MAX_GIT_COMMITS {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid Git history limit",
        ));
    }
    let cursor_head = cursor.map(|cursor| commit_id(&cursor.head)).transpose()?;
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error)
            if cursor.is_none()
                && error.code == ErrorCode::NotFound
                && !root.join(".git").exists() =>
        {
            return Ok(empty(RepositoryKind::Directory));
        }
        Err(error) => return Err(error),
    };
    let offset = cursor.map_or(0, |cursor| cursor.offset);
    let head = if let Some(head) = cursor_head {
        head
    } else {
        match repository.head() {
            Ok(head) => head.peel_to_commit().map_err(git_error)?.id(),
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => {
                return Ok(empty(RepositoryKind::Unborn));
            }
            Err(error) => return Err(git_error(error)),
        }
    };
    let (references, references_truncated) = references(&repository, control)?;
    let mut walk = repository.revwalk().map_err(git_error)?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(git_error)?;
    walk.push(head).map_err(git_error)?;
    let mut entries = Vec::with_capacity(limit);
    let mut truncated = false;
    for (position, oid) in walk.enumerate() {
        control.check()?;
        let oid = oid.map_err(git_error)?;
        if position < offset {
            continue;
        }
        if entries.len() == limit {
            truncated = true;
            break;
        }
        entries.push(entry(&repository, oid, references.get(&oid))?);
    }
    if entries.is_empty() && offset > 0 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "Git history offset is beyond the traversal",
        ));
    }
    Ok(GitLog {
        kind: RepositoryKind::Ready,
        head: Some(head.to_string()),
        offset,
        next: truncated.then(|| GitLogCursor {
            head: head.to_string(),
            offset: offset + entries.len(),
        }),
        entries,
        truncated,
        references_truncated,
    })
}

fn empty(kind: RepositoryKind) -> GitLog {
    GitLog {
        kind,
        head: None,
        offset: 0,
        next: None,
        entries: Vec::new(),
        truncated: false,
        references_truncated: false,
    }
}

fn references(
    repository: &Repository,
    control: &Control,
) -> Result<(HashMap<Oid, Vec<String>>, bool), Fault> {
    let mut references: HashMap<Oid, Vec<String>> = HashMap::new();
    let mut truncated = false;
    for (index, reference) in repository.references().map_err(git_error)?.enumerate() {
        control.check()?;
        if index == 256 {
            truncated = true;
            break;
        }
        let reference = reference.map_err(git_error)?;
        let Some(name) = reference.name_bytes().strip_prefix(b"refs/") else {
            continue;
        };
        let (name, partial) = bounded_text(name, 256);
        truncated |= partial;
        let Ok(resolved) = reference.resolve() else {
            continue;
        };
        let Ok(target) = resolved.peel_to_commit() else {
            continue;
        };
        let names = references.entry(target.id()).or_default();
        if names.len() < 8 {
            names.push(name);
            names.sort();
            names.dedup();
        } else {
            truncated = true;
        }
    }
    Ok((references, truncated))
}

fn entry(
    repository: &Repository,
    oid: Oid,
    references: Option<&Vec<String>>,
) -> Result<GitLogEntry, Fault> {
    let commit = repository.find_commit(oid).map_err(git_error)?;
    let signature = commit.author();
    let (message, message_partial) = bounded_text(commit.message_bytes(), 4096);
    let (author, author_partial) = bounded_text(signature.name_bytes(), 256);
    let (email, email_partial) = bounded_text(signature.email_bytes(), 256);
    Ok(GitLogEntry {
        id: oid.to_string(),
        message,
        author,
        email,
        timestamp: commit.time().seconds(),
        references: references.cloned().unwrap_or_default(),
        truncated: message_partial || author_partial || email_partial,
    })
}

fn commit_id(id: &str) -> Result<Oid, Fault> {
    // Accept an immutable object ID, not revision expressions or paths.
    if id.len() != 40 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected a full Git commit object ID",
        ));
    }
    Oid::from_str(id).map_err(git_error)
}

pub(super) fn commit(root: &Path, id: &str, control: &Control) -> Result<GitCommit, Fault> {
    let id = commit_id(id)?;
    let repository = open(root)?;
    let commit = repository.find_commit(id).map_err(git_error)?;
    let tree = commit.tree().map_err(git_error)?;
    let parent = if commit.parent_count() == 0 {
        None
    } else {
        Some(
            commit
                .parent(0)
                .map_err(git_error)?
                .tree()
                .map_err(git_error)?,
        )
    };
    control.check()?;
    let mut options = git2::DiffOptions::new();
    options.ignore_submodules(true).max_size(1024 * 1024);
    let diff = repository
        .diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))
        .map_err(git_error)?;
    let files = diff::files(&diff, control)?;
    Ok(GitCommit {
        entry: entry(&repository, id, None)?,
        additions: files.iter().map(|file| file.additions).sum(),
        deletions: files.iter().map(|file| file.deletions).sum(),
        binary: files.iter().any(|file| file.binary),
        truncated: files.iter().any(|file| file.truncated),
        files,
    })
}

fn bounded_text(bytes: &[u8], max: usize) -> (String, bool) {
    // Bound before lossy conversion so an oversized commit message does not
    // allocate another unbounded string just to display its prefix.
    let prefix = &bytes[..bytes.len().min(max)];
    let text = String::from_utf8_lossy(prefix);
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (
        text[..end].to_owned(),
        bytes.len() > max || std::str::from_utf8(prefix).is_err() || text.len() > max,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_utf8_text() {
        let (text, truncated) = bounded_text("中文提交".as_bytes(), 7);
        assert_eq!(text, "中文");
        assert!(truncated);
        let (text, truncated) = bounded_text(&[0xff; 20], 8);
        assert!(text.len() <= 8 && truncated);
        assert_eq!(bounded_text(b"commit", 8), ("commit".into(), false));
    }
}
