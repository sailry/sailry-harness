//! Turn diffs derive from confirmed writes, never from the current working tree.
use super::*;
use std::{collections::BTreeMap, path::Path};

pub(super) struct FileVersions {
    pub worktree: WorktreeId,
    pub segments: Vec<(Option<String>, String)>,
}

pub(super) struct Versions {
    pub files: BTreeMap<String, FileVersions>,
    pub partial: bool,
}

pub(super) async fn versions(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<Versions, Fault> {
    runs::visible(db, session, turn).await?;
    if matches!(
        runs::get(db, turn).await?.status,
        Status::Queued | Status::Running | Status::Stopping
    ) {
        return Err(Fault::new(ErrorCode::Busy, "turn diff is not final"));
    }
    let page = super::list(db, session, turn, None, 100).await?;
    let mut partial = page.next.is_some();
    let mut versions: BTreeMap<String, FileVersions> = BTreeMap::new();
    // Checkpoints are newest first. Only join adjacent versions when no external
    // edit intervened, so a manual edit between two tool writes is never attributed.
    for file in page.files.into_iter().rev() {
        match &file.outcome {
            RequestOutcome::Completed(result)
                if matches!(result.as_ref(), Ok(Output::FileWritten(written))
                if written.path == file.path && written.revision == file.after.revision && written.size == file.after.size) =>
                {}
            RequestOutcome::Unknown | RequestOutcome::Admitted => {
                partial = true;
                continue;
            }
            RequestOutcome::Completed(result) if matches!(result.as_ref(), Err(error) if error.code == ErrorCode::OutcomeUnknown) =>
            {
                partial = true;
                continue;
            }
            _ => continue,
        }
        if file
            .after
            .size
            .saturating_add(file.before.as_ref().map_or(0, |value| value.size))
            > (MAX_FRAME_BYTES / 8) as u64
        {
            partial = true;
            continue;
        }
        let content = super::reads::content(db, session, file.id).await?;
        let entry = versions.entry(file.path).or_insert_with(|| FileVersions {
            worktree: file.worktree,
            segments: Vec::new(),
        });
        if entry.worktree != file.worktree {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "turn file changed worktree",
            ));
        }
        let segments = &mut entry.segments;
        if let Some((_, after)) = segments.last_mut()
            && content.before.as_deref() == Some(after.as_str())
        {
            *after = content.after;
        } else {
            segments.push((content.before, content.after));
        }
    }
    Ok(Versions {
        files: versions,
        partial,
    })
}

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<checkpoint::TurnDiff, Fault> {
    let Versions {
        files: versions,
        mut partial,
    } = versions(db, session, turn).await?;
    let mut remaining = MAX_DIFF_BYTES;
    let mut files = Vec::new();
    for (path, versions) in versions {
        let mut file = GitDiff {
            path,
            scope: GitDiffScope::All,
            text: String::new(),
            additions: 0,
            deletions: 0,
            binary: false,
            truncated: false,
        };
        for (before, after) in versions.segments {
            append(&mut file, before.as_deref(), &after, &mut remaining)?;
        }
        partial |= file.truncated;
        if file.additions > 0 || file.deletions > 0 {
            files.push(file);
        }
    }
    Ok(checkpoint::TurnDiff {
        session,
        turn,
        files,
        partial,
    })
}

/// Appends one confirmed before/after segment as unified text within a shared byte budget.
pub(in crate::store) fn append(
    file: &mut GitDiff,
    before: Option<&str>,
    after: &str,
    remaining: &mut usize,
) -> Result<(), Fault> {
    if before == Some(after) {
        return Ok(());
    }
    let mut patch = git2::Patch::from_buffers(
        before.unwrap_or_default().as_bytes(),
        Some(Path::new(&file.path)),
        after.as_bytes(),
        Some(Path::new(&file.path)),
        None,
    )
    .map_err(storage_error)?;
    let (_, added, removed) = patch.line_stats().map_err(storage_error)?;
    file.additions += added;
    file.deletions += removed;
    let printed = patch.print(&mut |_, _, line| {
        let prefix = matches!(line.origin(), '+' | '-' | ' ');
        let text = String::from_utf8_lossy(line.content());
        let size = text.len() + usize::from(prefix);
        if size > *remaining {
            file.truncated = true;
            return false;
        }
        *remaining -= size;
        if prefix {
            file.text.push(line.origin());
        }
        file.text.push_str(&text);
        true
    });
    if !file.truncated {
        printed.map_err(storage_error)?;
    }
    Ok(())
}
