//! Repository grants retain the captured checkout and its registered project.
use rusqlite::Connection;
use sailry_protocol::{
    Command, ErrorCode, Fault, Worktree, WorktreeId,
    plugin::{Action, Context},
};

use crate::store::worktrees;

fn source(db: &Connection, context: &Context) -> Result<(Worktree, Vec<Worktree>), Fault> {
    let captured = context
        .worktree
        .ok_or_else(|| denied("repository action requires a worktree"))?;
    let entries = worktrees::list(db)?;
    let source = entries
        .iter()
        .find(|entry| entry.id == captured)
        .cloned()
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "captured worktree is unavailable"))?;
    Ok((source, entries))
}

fn in_project(source: &Worktree, target: WorktreeId, entries: &[Worktree]) -> bool {
    target == source.id
        || source.project.is_some()
            && entries
                .iter()
                .any(|entry| entry.id == target && entry.project == source.project)
}

pub(super) fn read_scope(
    db: &Connection,
    context: &Context,
    target: WorktreeId,
) -> Result<WorktreeId, Fault> {
    if context.worktree == Some(target) {
        return Ok(target);
    }
    if context.turn.is_none() {
        return Err(denied("Git read is outside the captured worktree"));
    }
    let (source, entries) = source(db, context)?;
    if !in_project(&source, target, &entries) {
        return Err(denied("Git read is outside the captured project"));
    }
    Ok(source.id)
}

pub(super) fn action(
    db: &Connection,
    context: &Context,
    command: &Command,
) -> Result<Action, Fault> {
    let (source, entries) = source(db, context)?;
    let project = source
        .project
        .ok_or_else(|| denied("worktree management requires a project"))?;
    let valid = match command {
        Command::ListWorktrees { worktree } => *worktree == source.id,
        Command::RegisterWorktree {
            project: target, ..
        }
        | Command::CreateWorktree {
            project: target, ..
        } => *target == project,
        Command::CreateManagedWorktree {
            project: target,
            source: target_source,
            ..
        } => *target == project && *target_source == source.id,
        Command::RemoveWorktree { worktree, .. } => in_project(&source, *worktree, &entries),
        _ => false,
    };
    if !valid {
        return Err(denied("worktree action is outside the captured project"));
    }
    Ok(if matches!(command, Command::ListWorktrees { .. }) {
        Action::ReadWorktrees
    } else {
        Action::WriteWorktrees
    })
}

fn denied(message: &str) -> Fault {
    Fault::new(ErrorCode::PermissionDenied, message)
}
