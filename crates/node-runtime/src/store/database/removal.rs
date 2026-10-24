//! Protect Node-owned resources during worktree deletion or project removal.
use super::*;
use std::path::PathBuf;

pub(in crate::store) struct Pending {
    caller: NodeId,
    request: RequestId,
    root: PathBuf,
    project: Option<ProjectId>,
}

impl Database {
    pub(super) fn check_removing(&self, id: WorktreeId) -> Result<(), Fault> {
        if self.removals.lock().unwrap().contains_key(&id) {
            Err(busy())
        } else {
            Ok(())
        }
    }

    pub(super) async fn check_removal_scope(&self, command: &Command) -> Result<(), Fault> {
        if self.removals.lock().unwrap().is_empty() && self.relocations.lock().unwrap().is_empty() {
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
            && let Ok(session) = super::super::commands::session(&self.connection, *session).await
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
                .lock()
                .unwrap()
                .values()
                .any(|pending| pending.project == Some(project))
        {
            return Err(busy());
        }
        if let Command::OpenTerminal { terminal, .. } = command {
            let info = super::super::terminals::required(&self.connection, *terminal).await?;
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
                .lock()
                .unwrap()
                .values()
                .any(|pending| pending.project.is_none() && path.starts_with(&pending.root))
                || self
                    .relocations
                    .lock()
                    .unwrap()
                    .values()
                    .flatten()
                    .any(|root| path.starts_with(root))
        }) {
            Err(busy())
        } else {
            Ok(())
        }
    }

    pub(super) async fn begin_removal(
        &mut self,
        caller: NodeId,
        request: &Request,
        id: WorktreeId,
        root: &Path,
    ) -> Result<(), Fault> {
        self.validate_removal(id, root).await?;
        self.removals.lock().unwrap().insert(
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

    pub(super) async fn begin_project_removal(
        &mut self,
        caller: NodeId,
        request: &Request,
        expected: &Project,
    ) -> Result<(), Fault> {
        let plan = super::super::projects::removal::prepare(&self.connection, expected).await?;
        self.guard_project(caller, request.id, expected.id, &plan.worktrees)
    }

    pub(in crate::store) async fn prepare_project_removal(
        &mut self,
        caller: NodeId,
        request: RequestId,
        expected: &Project,
    ) -> Result<Vec<terminal::Info>, Fault> {
        if !self.removals.lock().unwrap().values().any(|pending| {
            pending.caller == caller
                && pending.request == request
                && pending.project == Some(expected.id)
        }) {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "project removal is no longer pending",
            ));
        }
        let plan = super::super::projects::removal::prepare(&self.connection, expected).await?;
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
                .lock()
                .unwrap()
                .get(&tree.id)
                .is_some_and(|pending| pending.caller != caller || pending.request != request)
        }) {
            return Err(busy());
        }
        for tree in worktrees {
            self.removals.lock().unwrap().insert(
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

    pub(in crate::store) async fn project_terminal_closed(
        &mut self,
        worktree: WorktreeId,
        terminal: TerminalId,
        result: Result<terminal::Info, Fault>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<(), Fault> {
        let mut result = result.map(Output::Terminal);
        let publication = self.publication.clone();
        let changes = (async || {
            let transaction = self
                .connection
                .unchecked_transaction()
                .await
                .map_err(storage_error)?;
            let event = super::super::terminals::finish(
                &transaction,
                &Command::CloseTerminal { worktree, terminal },
                &mut result,
            )
            .await?;
            if result.is_err() {
                return Ok(None);
            }
            let info = super::super::terminals::required(&transaction, terminal).await?;
            if info.worktree != Some(worktree) || info.status != terminal::Status::Closed {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "terminal closure was not recorded",
                ));
            }
            let changes = record(&transaction, self.node, event.into_iter().collect()).await?;
            let publication = publication.lock().await;
            crate::store::database::conflict::commit(transaction).await?;
            Ok(Some((publication, changes)))
        })()
        .await
        .map_err(|_: Fault| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "terminal closure could not be recorded; inspect the terminal before retrying",
            )
        })?;
        result?;
        if let Some((publication, changes)) = changes {
            for event in changes {
                publication.send(events, event);
            }
        }
        Ok(())
    }

    pub(in crate::store) async fn validate_removal(
        &self,
        id: WorktreeId,
        root: &Path,
    ) -> Result<(), Fault> {
        let trees = super::super::worktrees::list(&self.connection).await?;
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
        let active: bool = {
            let mut statement = self
            .connection.prepare("SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
                 JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
                 WHERE r.worktree=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))").await.map_err(storage_error)?;
            let row = statement
                .query_row([id.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
            mapped.map_err(storage_error)?
        };
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
                .path
                .canonicalize()
                .ok()
                .is_some_and(|path| path.starts_with(root))
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree contains registered resources or Node storage",
            ));
        }
        if super::super::terminals::list(&self.connection)
            .await?
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
                .lock()
                .unwrap()
                .retain(|_, pending| pending.caller != caller || pending.request != request.id);
            return;
        }
        if let Command::RemoveWorktree { worktree, .. } = request.command {
            let mut removals = self.removals.lock().unwrap();
            if removals
                .get(&worktree)
                .is_some_and(|pending| pending.caller == caller && pending.request == request.id)
            {
                removals.remove(&worktree);
            }
        }
    }
}

fn busy() -> Fault {
    Fault::new(ErrorCode::Busy, "resource operation is pending")
}
