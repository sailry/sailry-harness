use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    thread::JoinHandle,
};

use sailry_link::Response;
use sailry_protocol::*;
use tokio::sync::{mpsc, oneshot};

use super::{CAPACITY, Job, external::Completed};
use crate::Error;

pub(super) struct Mutation {
    pub caller: NodeId,
    pub request: Request,
    pub roots: Roots,
    pub reply: oneshot::Sender<Response>,
}

pub(super) struct Roots {
    pub source: PathBuf,
    pub target: PathBuf,
}

impl Roots {
    pub fn entries(&self, command: &Command) -> Vec<(PathBuf, String)> {
        match command {
            Command::RenameEntry { from, to, .. }
            | Command::CopyEntry { from, to, .. }
            | Command::CopyEntryTo { from, to, .. }
            | Command::MoveEntryTo { from, to, .. } => vec![
                (self.source.clone(), from.clone()),
                (self.target.clone(), to.clone()),
            ],
            Command::TrashEntry { path, .. }
            | Command::TrashFile { path, .. }
            | Command::FinishFileUpload { path, .. } => {
                vec![(self.target.clone(), path.clone())]
            }
            _ => Vec::new(),
        }
    }
}

pub(super) struct Worker {
    sender: mpsc::Sender<Mutation>,
    thread: JoinHandle<()>,
    pub pending: usize,
}

impl Worker {
    pub fn start(
        completed: mpsc::Sender<Job>,
        execute: impl Fn(NodeId, &Roots, &Request) -> Response + Send + 'static,
    ) -> Result<Self, Error> {
        let (sender, mut receiver) = mpsc::channel::<Mutation>(CAPACITY);
        let thread = std::thread::Builder::new()
            .name("sailry-mutations".into())
            .spawn(move || {
                while let Some(mutation) = receiver.blocking_recv() {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        execute(mutation.caller, &mutation.roots, &mutation.request)
                    }))
                    .unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "resource operation panicked; inspect the target before retrying",
                        ))
                    });
                    // The store drains completions before closing its database and joining us.
                    if completed
                        .blocking_send(Job::Completed(Box::new(Completed {
                            caller: mutation.caller,
                            request: mutation.request,
                            result,
                            reply: mutation.reply,
                        })))
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            thread,
            pending: 0,
        })
    }

    pub fn submit(&mut self, mutation: Mutation) -> Result<(), Box<Mutation>> {
        // Bound admitted work including executing and not-yet-recorded completions.
        if self.pending >= CAPACITY {
            return Err(Box::new(mutation));
        }
        self.sender
            .try_send(mutation)
            .map_err(|error| Box::new(error.into_inner()))?;
        self.pending += 1;
        Ok(())
    }

    pub fn join(self) -> Result<(), Error> {
        drop(self.sender);
        self.thread
            .join()
            .map_err(|_| Error::Worker("resource worker panicked".into()))
    }
}

pub(super) enum Target {
    Plugin,
    Host,
    NewProject,
    ManagedWorktree {
        project: ProjectId,
        source: WorktreeId,
    },
    Worktree(WorktreeId),
    Worktrees {
        source: WorktreeId,
        target: WorktreeId,
    },
    Project(ProjectId),
}

pub(super) fn target(command: &Command) -> Option<Target> {
    match command {
        Command::InstallPluginUpload { .. }
        | Command::InstallPluginSource { .. }
        | Command::InstallBundledPlugin { .. }
        | Command::InstallSkill { .. }
        | Command::InstallMcp { .. }
        | Command::SavePluginMcp { .. } => Some(Target::Plugin),
        Command::ManageFiles(_)
        | Command::StopHostProcess { .. }
        | Command::OpenTerminal { .. }
        | Command::StopCommand { .. }
        | Command::StopCommands { .. } => Some(Target::Host),
        Command::CreateProject(_) => Some(Target::NewProject),
        Command::CreateManagedWorktree {
            project, source, ..
        } => Some(Target::ManagedWorktree {
            project: *project,
            source: *source,
        }),
        Command::CreateTerminal(launch) | Command::OpenToolTerminal { launch, .. } => {
            Some(Target::Worktree(launch.worktree))
        }
        Command::CloseTerminal { worktree, .. } => Some(Target::Worktree(*worktree)),
        Command::CopyEntryTo {
            source, worktree, ..
        }
        | Command::MoveEntryTo {
            source, worktree, ..
        } => Some(Target::Worktrees {
            source: *source,
            target: *worktree,
        }),
        Command::CreateWorktree { project, .. } | Command::RegisterWorktree { project, .. } => {
            Some(Target::Project(*project))
        }
        Command::ExportPdf { worktree, .. }
        | Command::WriteFile { worktree, .. }
        | Command::InstallPlugin { worktree, .. }
        | Command::RestoreFileCheckpoint { worktree, .. }
        | Command::FinishFileUpload { worktree, .. }
        | Command::FinishAttachmentUpload { worktree, .. }
        | Command::RemoveWorktree { worktree, .. }
        | Command::CreateDirectory { worktree, .. }
        | Command::RenameEntry { worktree, .. }
        | Command::CopyEntry { worktree, .. }
        | Command::TrashEntry { worktree, .. }
        | Command::TrashFile { worktree, .. }
        | Command::UpdateGitIndex { worktree, .. }
        | Command::CreateGitCommit { worktree, .. }
        | Command::RunGitAction { worktree, .. }
        | Command::CreateGitBranch { worktree, .. }
        | Command::RenameGitBranch { worktree, .. }
        | Command::DeleteGitBranch { worktree, .. }
        | Command::MergeGitBranch { worktree, .. }
        | Command::SwitchGitBranch { worktree, .. } => Some(Target::Worktree(*worktree)),
        _ => None,
    }
}

pub(super) fn execute(roots: &Roots, request: &Request) -> Response {
    let root = &roots.target;
    match &request.command {
        Command::StopHostProcess {
            pid,
            started_at_secs,
            force,
        } => crate::host::processes::stop(*pid, *started_at_secs, *force).map(|()| {
            Output::HostProcessSignalled {
                pid: *pid,
                started_at_secs: *started_at_secs,
            }
        }),
        Command::ManageFiles(action) => {
            crate::files::browser::execute(action).map(Output::FilesManaged)
        }
        Command::CreateProject(draft) => super::projects::execute(draft, &roots.target),
        Command::RemoveWorktree {
            worktree,
            expected_head,
            expected_branch,
        } => crate::git::worktrees::remove(root, expected_head, expected_branch)
            .map(|()| Output::WorktreeRemoved { id: *worktree }),
        Command::MergeGitBranch {
            name,
            commit,
            expected_head,
            expected_branch,
            expected_index,
            ..
        } => crate::git::merge::branch(
            root,
            name,
            commit,
            expected_head,
            expected_branch,
            expected_index,
        )
        .map(Output::GitMerged),
        Command::RegisterWorktree { project, path } => crate::git::worktrees::resolve(root, path)
            .map(|path| {
                Output::Worktree(Worktree {
                    id: WorktreeId::new(),
                    project: Some(*project),
                    path,
                    main: false,
                })
            }),
        Command::CreateWorktree {
            project,
            path,
            branch,
            commit,
        } => {
            let id = WorktreeId::new();
            crate::git::worktrees::create(root, path, branch, commit, &id.to_string()).map(|path| {
                Output::Worktree(Worktree {
                    id,
                    project: Some(*project),
                    path,
                    main: false,
                })
            })
        }
        Command::CreateManagedWorktree {
            project,
            branch,
            expected_head,
            expected_index,
            include_changes,
            ..
        } => {
            let id = WorktreeId::new();
            crate::git::worktrees::managed(
                &roots.source,
                root,
                branch,
                expected_head,
                expected_index,
                *include_changes,
                &id.to_string(),
            )
            .map(|path| {
                Output::Worktree(Worktree {
                    id,
                    project: Some(*project),
                    path,
                    main: false,
                })
            })
        }
        Command::RenameGitBranch {
            name,
            new_name,
            commit,
            ..
        } => {
            crate::git::branches::rename(root, name, new_name, commit).map(Output::GitBranchRenamed)
        }
        Command::DeleteGitBranch {
            name,
            commit,
            expected_head,
            expected_branch,
            ..
        } => crate::git::branches::remove(
            root,
            name,
            commit,
            expected_head,
            expected_branch.as_deref(),
        )
        .map(|()| Output::GitBranchDeleted { name: name.clone() }),
        Command::SwitchGitBranch {
            name,
            commit,
            expected_head,
            expected_branch,
            expected_index,
            ..
        } => crate::git::checkout::switch(
            root,
            name,
            commit,
            expected_head.as_deref(),
            expected_branch.as_deref(),
            expected_index,
        )
        .map(Output::GitBranchSwitched),
        Command::CreateGitBranch { name, commit, .. } => {
            crate::git::branches::create(root, name, commit).map(Output::GitBranchCreated)
        }
        Command::CreateDirectory { path, .. } => crate::files::directory(root, path)
            .map(|()| Output::DirectoryCreated { path: path.clone() }),
        Command::RenameEntry { from, to, .. } => {
            crate::files::rename(root, from, to).map(|()| Output::EntryRenamed {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::MoveEntryTo { from, to, .. } => {
            crate::files::rename::between(&roots.source, root, from, to).map(|()| {
                Output::EntryMoved {
                    from: from.clone(),
                    to: to.clone(),
                }
            })
        }
        Command::TrashEntry { path, .. } => {
            crate::files::trash(root, path).map(|()| Output::EntryTrashed { path: path.clone() })
        }
        Command::TrashFile {
            path,
            expected_revision,
            expected_stamp,
            ..
        } => crate::files::remove_verified(root, path, expected_revision, expected_stamp)
            .map(|()| Output::EntryTrashed { path: path.clone() }),
        Command::CopyEntry { from, to, .. } => {
            crate::files::copy::entry(root, from, to).map(|()| Output::EntryCopied {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::CopyEntryTo { from, to, .. } => {
            crate::files::copy::between(&roots.source, root, from, to).map(|()| {
                Output::EntryCopied {
                    from: from.clone(),
                    to: to.clone(),
                }
            })
        }
        Command::ExportPdf { options, .. } => {
            crate::office::export(root, options).map(Output::OfficeWritten)
        }
        Command::WriteFile {
            path,
            text,
            expected_revision,
            ..
        } => crate::files::save(root, path, text, expected_revision.as_deref())
            .map(Output::FileWritten),
        Command::UpdateGitIndex {
            paths,
            operation,
            expected_index,
            expected_head,
            ..
        } => crate::git::index::update(
            root,
            paths,
            *operation,
            expected_index,
            expected_head.as_deref(),
            request.id,
        )
        .map(Output::GitIndex),
        Command::RunGitAction {
            action,
            expected_index,
            expected_head,
            expected_branch,
            ..
        } => crate::git::actions::execute(
            root,
            action,
            expected_index,
            expected_head.as_deref(),
            expected_branch.as_deref(),
        )
        .map(|()| Output::GitActionCompleted),
        Command::CreateGitCommit {
            message,
            options,
            amend,
            expected_index,
            expected_head,
            expected_branch,
            ..
        } => crate::git::commit::create(
            root,
            message,
            *amend,
            options,
            expected_index,
            expected_head.as_deref(),
            expected_branch.as_deref(),
        )
        .map(|(id, follow_up)| Output::GitCommitCreated { id, follow_up }),
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "resource mutation expected",
        )),
    }
}

#[cfg(test)]
mod tests;
