//! Protect Node-owned resources during worktree deletion or project removal.
use super::*;
use std::path::PathBuf;

pub(super) struct Pending {
    caller: NodeId,
    request: RequestId,
    root: PathBuf,
    project: Option<ProjectId>,
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
        // Retained sessions cannot admit turns while their checkout is being removed.
        if let Command::QueueTurn { session, .. }
        | Command::SubmitTurn { session, .. }
        | Command::SubmitTask { session, .. }
        | Command::ContinueTurn { session, .. }
        | Command::CompactContext { session, .. } = command
            && let Ok(session) = super::super::commands::session(&self.connection, *session)
        {
            self.check_removing(session.worktree)?;
        }
        let project = match command {
            Command::CreateSession { project, .. } => *project,
            Command::CreateWorktree { project, .. }
            | Command::RegisterWorktree { project, .. }
            | Command::CreateManagedWorktree { project, .. } => Some(*project),
            Command::UpdateProject { expected, .. } | Command::RemoveProject { expected } => {
                Some(expected.id)
            }
            _ => None,
        };
        if let Some(project) = project
            && self
                .removals
                .values()
                .any(|pending| pending.project == Some(project))
        {
            return Err(busy());
        }
        if let Command::OpenTerminal { terminal, .. } = command {
            let info = super::super::terminals::required(&self.connection, *terminal)?;
            if let Some(id) = info.worktree {
                self.check_removing(id)?;
            }
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
                .any(|pending| pending.project.is_none() && path.starts_with(&pending.root))
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
                project: None,
            },
        );
        Ok(())
    }

    pub(super) fn begin_project_removal(
        &mut self,
        caller: NodeId,
        request: &Request,
        expected: &Project,
    ) -> Result<(), Fault> {
        let plan = super::super::projects::removal::prepare(&self.connection, expected)?;
        self.guard_project(caller, request.id, expected.id, &plan.worktrees)
    }

    pub(in crate::store) fn prepare_project_removal(
        &mut self,
        caller: NodeId,
        request: RequestId,
        expected: &Project,
    ) -> Result<Vec<terminal::Info>, Fault> {
        if !self.removals.values().any(|pending| {
            pending.caller == caller
                && pending.request == request
                && pending.project == Some(expected.id)
        }) {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "project removal is no longer pending",
            ));
        }
        let plan = super::super::projects::removal::prepare(&self.connection, expected)?;
        // Earlier admitted worktree registrations finish before the worker prepares again.
        self.guard_project(caller, request, expected.id, &plan.worktrees)?;
        Ok(plan.terminals)
    }

    fn guard_project(
        &mut self,
        caller: NodeId,
        request: RequestId,
        project: ProjectId,
        worktrees: &[Worktree],
    ) -> Result<(), Fault> {
        if worktrees.iter().any(|tree| {
            self.removals
                .get(&tree.id)
                .is_some_and(|pending| pending.caller != caller || pending.request != request)
        }) {
            return Err(busy());
        }
        for tree in worktrees {
            self.removals.insert(
                tree.id,
                Pending {
                    caller,
                    request,
                    root: tree.path.clone().into(),
                    project: Some(project),
                },
            );
        }
        Ok(())
    }

    pub(in crate::store) fn project_terminal_closed(
        &mut self,
        worktree: WorktreeId,
        terminal: TerminalId,
        result: Result<terminal::Info, Fault>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<(), Fault> {
        let mut result = result.map(Output::Terminal);
        let changes = (|| {
            let transaction = self.connection.transaction().map_err(storage_error)?;
            let event = super::super::terminals::finish(
                &transaction,
                &Command::CloseTerminal { worktree, terminal },
                &mut result,
            )?;
            if result.is_err() {
                return Ok(Vec::new());
            }
            let info = super::super::terminals::required(&transaction, terminal)?;
            if info.worktree != Some(worktree) || info.status != terminal::Status::Closed {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "terminal closure was not recorded",
                ));
            }
            let changes = record(&transaction, self.node, event.into_iter().collect())?;
            transaction.commit().map_err(storage_error)?;
            Ok(changes)
        })()
        .map_err(|_: Fault| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "terminal closure could not be recorded; inspect the terminal before retrying",
            )
        })?;
        result?;
        for event in changes {
            let _ = events.send(event);
        }
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
        // Idle sessions keep their history after removal; active turns still need the checkout.
        let active: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
                 JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
                 WHERE r.worktree=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
                [id.to_string()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if active {
            return Err(Fault::new(
                ErrorCode::Busy,
                "worktree has active or queued turns",
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
        if matches!(request.command, Command::RemoveProject { .. }) {
            self.removals
                .retain(|_, pending| pending.caller != caller || pending.request != request.id);
            return;
        }
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
    Fault::new(ErrorCode::Busy, "resource operation is pending")
}
