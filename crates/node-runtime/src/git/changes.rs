//! Metadata inventory and frozen, literal per-file diffs share one captured target.
use std::path::Path;

use sailry_protocol::*;

use super::{Control, open};

mod contents;
mod inventory;
#[cfg(test)]
mod tests;

pub(super) struct Page<'a> {
    pub expected_revision: Option<&'a str>,
    pub offset: usize,
    pub limit: usize,
}

pub(super) fn list(
    root: &Path,
    target: Option<GitChangeTarget>,
    cursor: Option<GitChangesCursor>,
    limit: usize,
    control: &Control,
) -> Result<GitChanges, Fault> {
    validate_page(0, limit)?;
    let repository = open(root)?;
    let (capture, after) = match (target, cursor) {
        (Some(target), None) => (inventory::start(root, &repository, target, control)?, None),
        (None, Some(cursor)) => {
            let capture = inventory::restore(root, &repository, &cursor.snapshot, control)?;
            if let Some(path) = &cursor.after_path {
                super::valid_path(path)?;
                if !capture.entries.contains_key(path) {
                    return Err(Fault::invalid(
                        "Git change cursor path is outside the snapshot",
                    ));
                }
            }
            (capture, cursor.after_path)
        }
        _ => {
            return Err(Fault::invalid(
                "provide exactly one Git change target or cursor",
            ));
        }
    };
    let mut result = GitChanges {
        snapshot: capture.snapshot.clone(),
        entries: Vec::new(),
        next: None,
        exhausted: false,
        omitted_paths: capture.omitted,
    };
    let candidates: Vec<_> = capture
        .entries
        .values()
        .filter(|candidate| {
            after
                .as_ref()
                .is_none_or(|path| &candidate.entry.path > path)
        })
        .collect();
    for (position, candidate) in candidates.iter().enumerate() {
        control.check()?;
        result.entries.push(candidate.entry.clone());
        result.next = (position + 1 < candidates.len()).then(|| GitChangesCursor {
            snapshot: result.snapshot.clone(),
            after_path: Some(candidate.entry.path.clone()),
        });
        result.exhausted = result.next.is_none() && result.omitted_paths == 0;
        if encoded(&Output::GitChanges(result.clone()))? > MAX_DIFF_BYTES {
            result.entries.pop();
            let Some(last) = result.entries.last() else {
                return Err(Fault::invalid(
                    "Git change metadata exceeds the page size limit",
                ));
            };
            result.next = Some(GitChangesCursor {
                snapshot: result.snapshot.clone(),
                after_path: Some(last.path.clone()),
            });
            result.exhausted = false;
            break;
        }
        if result.entries.len() == limit.min(MAX_GIT_LINES) {
            break;
        }
    }
    if candidates.is_empty() {
        result.exhausted = result.omitted_paths == 0;
    }
    Ok(result)
}

pub(super) fn read(
    root: &Path,
    snapshot: GitChangeSnapshot,
    path: &str,
    expected_metadata: &str,
    mut page: Page<'_>,
    control: &Control,
) -> Result<GitChange, Fault> {
    validate_page(page.offset, page.limit)?;
    page.limit = page.limit.min(MAX_GIT_LINES);
    super::valid_path(path)?;
    digest(expected_metadata)?;
    if let Some(revision) = page.expected_revision {
        digest(revision)?;
    }
    if page.offset > 0 && page.expected_revision.is_none() {
        return Err(Fault::invalid(
            "Git change continuation requires the content revision",
        ));
    }
    let repository = open(root)?;
    let capture = inventory::restore(root, &repository, &snapshot, control)?;
    let candidate = capture
        .entries
        .get(path)
        .ok_or_else(|| Fault::invalid("Git change path is outside the snapshot"))?;
    if candidate.entry.metadata_revision != expected_metadata {
        return Err(stale());
    }
    let result = contents::read(root, &repository, &snapshot, candidate, page, control)?;
    // Detect branch, index, or stat changes during the saved-buffer read and diff.
    inventory::restore(root, &repository, &snapshot, control)?;
    Ok(result)
}

fn validate_page(offset: usize, limit: usize) -> Result<(), Fault> {
    if limit == 0 || offset.checked_add(limit).is_none() {
        return Err(Fault::invalid("invalid Git change page"));
    }
    Ok(())
}

fn digest(value: &str) -> Result<(), Fault> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::invalid("expected a full Git change revision"));
    }
    Ok(())
}

fn hash(value: &impl serde::Serialize) -> Result<String, Fault> {
    serde_json::to_vec(value)
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .map_err(|_| {
            Fault::new(
                ErrorCode::Internal,
                "Git change metadata could not be encoded",
            )
        })
}

fn encoded(value: &impl serde::Serialize) -> Result<usize, Fault> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| {
            Fault::new(
                ErrorCode::Internal,
                "Git change result could not be encoded",
            )
        })
}

fn stale() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "Git changes changed; start a new review",
    )
}
