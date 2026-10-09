use super::*;
use sailry_protocol::conversation::checkpoint::hunk;
use std::path::Path;
use tokio::sync::mpsc;

mod content;
pub(in crate::store) use content::Change;

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    path: &str,
) -> Result<hunk::Review, Fault> {
    crate::git::valid_path(path)?;
    let versions = diff::versions(db, session, turn)?;
    let file = versions.files.get(path).ok_or_else(missing)?;
    let mut remaining = MAX_DIFF_BYTES;
    let mut partial = versions.partial;
    let mut hunks = Vec::new();
    for (segment, (before, after)) in file.segments.iter().enumerate() {
        let changes =
            match content::split(path, before.as_deref().unwrap_or_default(), after, segment) {
                Ok(changes) => changes,
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
        for change in changes {
            if change.text.len() > remaining || hunks.len() == 100 {
                partial = true;
                continue;
            }
            remaining -= change.text.len();
            hunks.push(hunk::Hunk {
                id: change.id,
                old_start: change.old_start,
                new_start: change.new_start,
                text: change.text,
                additions: change.additions,
                deletions: change.deletions,
            });
        }
    }
    Ok(hunk::Review {
        session,
        turn,
        worktree: file.worktree,
        path: path.to_owned(),
        hunks,
        partial,
    })
}

pub(in crate::store) fn prepare(
    database: &Database,
    selection: &hunk::Selection,
    root: &Path,
) -> Result<Change, Fault> {
    crate::git::valid_path(&selection.path)?;
    if selection.hunk.len() != 64
        || !selection.hunk.bytes().all(|byte| byte.is_ascii_hexdigit())
        || selection.expected_revision.len() != 64
        || !selection
            .expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || (selection.operation == hunk::Operation::Stage && selection.expected_index.is_none())
        || (selection.operation == hunk::Operation::Reject
            && (selection.expected_index.is_some() || selection.expected_head.is_some()))
    {
        return Err(Fault::invalid("invalid turn hunk selection"));
    }
    let versions = diff::versions(&database.connection, selection.session, selection.turn)?;
    let file = versions.files.get(&selection.path).ok_or_else(missing)?;
    if file.worktree != selection.worktree || database.worktree_root(selection.worktree)? != root {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "turn hunk belongs to another worktree",
        ));
    }
    database.validate_relocation(&[(root.to_owned(), selection.path.clone())])?;
    for (segment, (before, after)) in file.segments.iter().enumerate() {
        let Ok(changes) = content::split(
            &selection.path,
            before.as_deref().unwrap_or_default(),
            after,
            segment,
        ) else {
            continue;
        };
        for change in changes {
            if change.id == selection.hunk {
                return Ok(change);
            }
        }
    }
    Err(missing())
}

pub(in crate::store) fn execute(
    sender: &mpsc::Sender<Job>,
    root: &Path,
    selection: &hunk::Selection,
    id: RequestId,
) -> Result<hunk::Applied, Fault> {
    let (reply, response) = std::sync::mpsc::channel();
    sender
        .blocking_send(Job::PrepareHunk {
            selection: selection.clone(),
            root: root.to_owned(),
            reply,
        })
        .map_err(|_| crate::store::unavailable())?;
    let change = response.recv().map_err(|_| crate::store::unavailable())??;
    let current = crate::files::capture_text(root, &selection.path, &selection.expected_revision)?;
    let (revision, index) = match selection.operation {
        hunk::Operation::Reject => {
            let text = content::apply(&current, &change.after, &change.before)?;
            let written = crate::files::restore_text(
                root,
                &selection.path,
                &text,
                &selection.expected_revision,
            )?;
            (written.revision, None)
        }
        hunk::Operation::Stage => {
            // Validate the selected source still exists before touching the index.
            content::apply(&current, &change.after, &change.before)?;
            let index = crate::git::index::edit(
                root,
                &selection.path,
                selection.expected_index.as_deref().unwrap(),
                selection.expected_head.as_deref(),
                id,
                &selection.expected_revision,
                |indexed| content::apply(indexed, &change.before, &change.after),
            )?;
            (selection.expected_revision.clone(), Some(index))
        }
    };
    Ok(hunk::Applied {
        session: selection.session,
        turn: selection.turn,
        path: selection.path.clone(),
        hunk: selection.hunk.clone(),
        revision,
        index,
    })
}

fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "confirmed turn hunk does not exist")
}
