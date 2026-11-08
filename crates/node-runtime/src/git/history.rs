//! Bounded HEAD traversal and reference labels adapted from sailry-code
//! 67ae9fa0 sailry-git/src/action_read/log.rs (Apache-2.0).
use std::{collections::HashMap, path::Path};

use git2::{Oid, Repository, Sort};
use sailry_protocol::*;

use super::{Control, diff, fault, open};

pub(super) fn log(
    root: &Path,
    limit: usize,
    cursor: Option<&GitLogCursor>,
    revision: Option<&str>,
    path: Option<&str>,
    control: &Control,
) -> Result<GitLog, Fault> {
    if limit == 0 {
        return Err(Fault::invalid("Git history limit must be positive"));
    }
    let page_limit = limit.min(MAX_GIT_COMMITS);
    let cursor_head = cursor.map(|cursor| commit_id(&cursor.head)).transpose()?;
    let after = cursor.map(|cursor| commit_id(&cursor.after)).transpose()?;
    if let Some(path) = path {
        super::valid_path(path)?;
    }
    if cursor.is_some_and(|cursor| cursor.path.as_deref() != path) {
        return Err(Fault::invalid("Git history cursor belongs to another path"));
    }
    if revision.is_some_and(|revision| {
        revision.is_empty() || revision.len() > 1024 || revision.chars().any(char::is_control)
    }) {
        return Err(Fault::invalid("invalid Git revision"));
    }
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error)
            if cursor.is_none()
                && revision.is_none()
                && error.code == ErrorCode::NotFound
                && !root.join(".git").exists() =>
        {
            return Ok(empty(RepositoryKind::Directory));
        }
        Err(error) => return Err(error),
    };
    let head = if let Some(head) = cursor_head {
        head
    } else if let Some(revision) = revision {
        repository
            .revparse_single(revision)
            .map_err(fault)?
            .peel_to_commit()
            .map_err(fault)?
            .id()
    } else {
        match repository.head() {
            Ok(head) => head.peel_to_commit().map_err(fault)?.id(),
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => {
                return Ok(empty(RepositoryKind::Unborn));
            }
            Err(error) => return Err(fault(error)),
        }
    };
    let (references, references_truncated) = references(&repository, control)?;
    let mut walk = repository.revwalk().map_err(fault)?;
    walk.set_sorting(Sort::TIME).map_err(fault)?;
    walk.push(head).map_err(fault)?;
    let offset = cursor.map_or(0, |cursor| cursor.offset);
    let mut resumed = after.is_none();
    let mut truncated = false;
    let mut entries = Vec::with_capacity(page_limit);
    for oid in walk {
        control.check()?;
        let oid = oid.map_err(fault)?;
        #[cfg(test)]
        tests::VISITED.with(|visited| visited.set(visited.get() + 1));
        if !resumed {
            resumed = Some(oid) == after;
            continue;
        }
        if !path
            .map(|path| touches(&repository, oid, path))
            .transpose()?
            .unwrap_or(true)
        {
            continue;
        }
        if entries.len() == page_limit {
            truncated = true;
            break;
        }
        entries.push(entry(&repository, oid, references.get(&oid))?);
    }
    if !resumed {
        return Err(Fault::invalid(
            "Git history cursor does not match the traversal",
        ));
    }
    if entries.is_empty() && offset > 0 {
        return Err(Fault::invalid("Git history offset is beyond the traversal"));
    }
    Ok(GitLog {
        kind: RepositoryKind::Ready,
        head: Some(head.to_string()),
        offset,
        next: truncated.then(|| GitLogCursor {
            head: head.to_string(),
            offset: offset.saturating_add(entries.len()),
            path: path.map(str::to_owned),
            after: entries
                .last()
                .expect("nonempty Git history page")
                .id
                .clone(),
        }),
        entries,
        truncated,
        references_truncated,
    })
}

#[cfg(test)]
mod tests;

fn touches(repository: &Repository, oid: Oid, path: &str) -> Result<bool, Fault> {
    #[cfg(test)]
    tests::COMPARED.with(|compared| compared.set(compared.get() + 1));
    let commit = repository.find_commit(oid).map_err(fault)?;
    let entry = |tree: git2::Tree<'_>| match tree.get_path(Path::new(path)) {
        Ok(entry) => Ok(Some((entry.id(), entry.filemode()))),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(fault(error)),
    };
    let current = entry(commit.tree().map_err(fault)?)?;
    let previous = if commit.parent_count() == 0 {
        None
    } else {
        entry(commit.parent(0).map_err(fault)?.tree().map_err(fault)?)?
    };
    Ok(current != previous)
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
    for (index, reference) in repository.references().map_err(fault)?.enumerate() {
        control.check()?;
        if index == 256 {
            truncated = true;
            break;
        }
        let reference = reference.map_err(fault)?;
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
    let commit = repository.find_commit(oid).map_err(fault)?;
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
        return Err(Fault::invalid("expected a full Git commit object ID"));
    }
    Oid::from_str(id).map_err(fault)
}

pub(super) fn commit(root: &Path, id: &str, control: &Control) -> Result<GitCommit, Fault> {
    let id = commit_id(id)?;
    let repository = open(root)?;
    let commit = repository.find_commit(id).map_err(fault)?;
    let tree = commit.tree().map_err(fault)?;
    let parent = if commit.parent_count() == 0 {
        None
    } else {
        Some(commit.parent(0).map_err(fault)?.tree().map_err(fault)?)
    };
    control.check()?;
    let mut options = git2::DiffOptions::new();
    options.ignore_submodules(false).max_size(1024 * 1024);
    let diff = repository
        .diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))
        .map_err(fault)?;
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
