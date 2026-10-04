//! Protect Node-owned resources while the existing mutation worker removes a worktree.
use super::*;
use std::path::PathBuf;

pub(super) struct Pending {
    caller: NodeId,
    request: RequestId,
    root: PathBuf,
}

impl Database {
    pub(super) fn check_removing(&self, id: WorktreeId) -> Result<(), Fault> {
        if self.removals.contains_key(&id) {
            Err(busy())
        } else {
            Ok(())
        }
    }

    pub(super) fn check_removal_scope(&self, command: &Command) -> Result<(), Fault> {
        if self.removals.is_empty() && self.relocations.is_empty() {
            return Ok(());
        }
        if let Command::CreateSession {
            worktree: Some(id), ..
        }
        | Command::MoveConversation { worktree: id, .. }
        | Command::ForkConversationAt { worktree: id, .. }
        | Command::CreateManagedWorktree { source: id, .. } = command
        {
            self.check_removing(*id)?;
        }
        if let Command::ImportSession(bundle) = command
            && let Some(id) = bundle.worktree
        {
            self.check_removing(id)?;
        }
        // Do not introduce registered resources inside a directory being relocated.
        let path = match command {
            Command::CreateProject(draft) => Path::new(&draft.path),
            Command::RegisterProject { path, .. }
            | Command::RegisterWorktree { path, .. }
            | Command::CreateWorktree { path, .. } => Path::new(path),
            _ => return Ok(()),
        };
        let resolved = path.canonicalize().or_else(|error| {
            let Some(parent) = path.parent() else {
                return Err(error);
            };
            let Some(name) = path.file_name() else {
                return Err(error);
            };
            parent.canonicalize().map(|parent| parent.join(name))
        });
        if resolved.is_ok_and(|path| {
            self.removals
                .values()
                .any(|pending| path.starts_with(&pending.root))
                || self
                    .relocations
                    .values()
                    .flatten()
                    .any(|root| path.starts_with(root))
        }) {
            Err(busy())
        } else {
            Ok(())
        }
    }

    pub(super) fn begin_removal(
        &mut self,
        caller: NodeId,
        request: &Request,
        id: WorktreeId,
        root: &Path,
    ) -> Result<(), Fault> {
        self.validate_removal(id, root)?;
        self.removals.insert(
            id,
            Pending {
                caller,
                request: request.id,
                root: root.to_owned(),
            },
        );
        Ok(())
    }

    pub(in crate::store) fn validate_removal(
        &self,
        id: WorktreeId,
        root: &Path,
    ) -> Result<(), Fault> {
        let trees = super::super::worktrees::list(&self.connection)?;
        let tree = trees.iter().find(|tree| tree.id == id).ok_or_else(|| {
            Fault::new(ErrorCode::NotFound, "worktree does not exist on this Node")
        })?;
        if tree.main {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "cannot remove the main worktree",
            ));
        }
        if Path::new(&tree.path) != root {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "worktree registration changed",
            ));
        }
        let in_use: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM session_revisions WHERE worktree=?1)",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if in_use {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree still owns sessions",
            ));
        }
        if trees
            .iter()
            .any(|tree| tree.id != id && Path::new(&tree.path).starts_with(root))
            || self
                .connection
                .path()
                .and_then(|path| Path::new(path).canonicalize().ok())
                .is_some_and(|path| path.starts_with(root))
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree contains registered resources or Node storage",
            ));
        }
        if super::super::terminals::list(&self.connection)?
            .iter()
            .any(|info| info.worktree == Some(id) && info.status == terminal::Status::Running)
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree still owns running terminals",
            ));
        }
        Ok(())
    }

    pub(super) fn end_removal(&mut self, caller: NodeId, request: &Request) {
        if let Command::RemoveWorktree { worktree, .. } = request.command
            && self
                .removals
                .get(&worktree)
                .is_some_and(|pending| pending.caller == caller && pending.request == request.id)
        {
            self.removals.remove(&worktree);
        }
    }
}

fn busy() -> Fault {
    Fault::new(ErrorCode::Busy, "resource path is changing")
}
