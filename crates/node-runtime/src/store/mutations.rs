use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::Arc,
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
    pub access: crate::files::Access,
    pub environment: Arc<crate::process::Environment>,
}

impl Roots {
    fn changed(
        &self,
        command: &Command,
        output: &Output,
        watches: &crate::files::watch::Watches,
        index: &crate::code_index::Service,
    ) {
        for (root, path) in self.entries(command) {
            index.invalidate(&root, Some(&path));
        }
        match output {
            Output::FileWritten(file) => index.invalidate(&self.target, Some(&file.path)),
            Output::FileRestored(file) => index.invalidate(&self.target, Some(&file.path)),
            _ => {}
        }
        let paths = match output {
            Output::FileWritten(file) => vec![(&self.target, file.path.as_str())],
            Output::FileRestored(file) => vec![(&self.target, file.path.as_str())],
            Output::DirectoryCreated { path } | Output::EntryTrashed { path } => {
                vec![(&self.target, path.as_str())]
            }
            Output::EntryRenamed { from, to } => {
                vec![(&self.target, from.as_str()), (&self.target, to.as_str())]
            }
            Output::EntryCopied { to, .. } => vec![(&self.target, to.as_str())],
            Output::EntryMoved { from, to } => {
                vec![(&self.source, from.as_str()), (&self.target, to.as_str())]
            }
            _ => Vec::new(),
        };
        let mut roots = std::collections::BTreeMap::<_, Vec<_>>::new();
        for (root, path) in paths {
            roots.entry(root).or_default().push(root.join(path));
        }
        for (root, paths) in roots {
            watches.changed_paths(root, &paths);
        }
    }

    pub fn entries(&self, command: &Command) -> Vec<(PathBuf, String)> {
        match command {
            Command::RenameEntry { from, to, .. }
            | Command::MoveFile { from, to, .. }
            | Command::CopyEntry { from, to, .. }
            | Command::CopyEntryTo { from, to, .. }
            | Command::MoveEntryTo { from, to, .. } => vec![
                (self.source.clone(), from.clone()),
                (self.target.clone(), to.clone()),
            ],
            Command::CreateDirectory { path, .. }
                if self.access == crate::files::Access::Manual
                    && crate::files::path::components_with(path, false, self.access).is_ok_and(
                        |parts| {
                            parts.len() >= 2 && crate::files::path::worktrees(parts[0], parts[1])
                        },
                    ) =>
            {
                vec![(self.target.clone(), path.clone())]
            }
            Command::TrashEntry { path, .. }
            | Command::DeleteFile { path, .. }
            | Command::TrashFile { path, .. }
            | Command::WriteFile { path, .. }
            | Command::EditFile { path, .. }
            | Command::FinishFileUpload { path, .. } => {
                vec![(self.target.clone(), path.clone())]
            }
            Command::EditNotebook { edit, .. } => vec![(self.target.clone(), edit.path.clone())],
            Command::ApplyTurnHunk(selection) => {
                vec![(self.target.clone(), selection.path.clone())]
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
        watches: Arc<crate::files::watch::Watches>,
        index: Arc<crate::code_index::Service>,
        git: crate::git::Git,
        execute: impl Fn(NodeId, &Roots, &Request) -> Response + Send + 'static,
    ) -> Result<Self, Error> {
        let (sender, mut receiver) = mpsc::channel::<Mutation>(CAPACITY);
        let thread = std::thread::Builder::new()
            .name("sailry-mutations".into())
            .spawn(move || {
                while let Some(mutation) = receiver.blocking_recv() {
                    git.invalidate(&mutation.roots.source);
                    git.invalidate(&mutation.roots.target);
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        execute(mutation.caller, &mutation.roots, &mutation.request)
                    }))
                    .unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "resource operation panicked; inspect the target before retrying",
                        ))
                    });
                    // Later reads must not join a scan that overlapped this write,
                    // including uncertain effects and early-return command paths.
                    git.invalidate(&mutation.roots.source);
                    git.invalidate(&mutation.roots.target);
                    if let Ok(output) = &result {
                        // A completed physical change remains visible if receipt persistence fails.
                        mutation
                            .roots
                            .changed(&mutation.request.command, output, &watches, &index);
                    }
                    if matches!(
                        mutation.request.command,
                        Command::RunGitAction { .. }
                            | Command::CreateGitBranch { .. }
                            | Command::RenameGitBranch { .. }
                            | Command::DeleteGitBranch { .. }
                            | Command::MergeGitBranch { .. }
                            | Command::SwitchGitBranch { .. }
                    ) {
                        index.invalidate(&mutation.roots.target, None);
                        watches.changed(&mutation.roots.target);
                    }
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
        | Command::InstallRegistrySkill { .. }
        | Command::InstallSkillUpload { .. }
        | Command::InstallMcp { .. }
        | Command::SavePluginMcp { .. } => Some(Target::Plugin),
        Command::SetPluginEnabled { enabled: true, .. } => Some(Target::Plugin),
        Command::ManageFiles(_)
        | Command::RemoveProject { .. }
        | Command::StopHostProcess { .. }
        | Command::OpenTerminal { .. }
        | Command::StopCommand { .. }
        | Command::WriteCommand { .. }
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
        Command::WriteFile { worktree, .. }
        | Command::EditFile { worktree, .. }
        | Command::EditNotebook { worktree, .. }
        | Command::DeleteFile { worktree, .. }
        | Command::MoveFile { worktree, .. }
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
        Command::ApplyTurnHunk(selection) => Some(Target::Worktree(selection.worktree)),
        _ => None,
    }
}

pub(super) fn execute(roots: &Roots, request: &Request) -> Response {
    let root = &roots.target;
    let scope = roots.access.scope(root);
    let source = roots.access.scope(&roots.source);
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
        Command::CreateDirectory { path, .. } => crate::files::directory(scope, path)
            .map(|()| Output::DirectoryCreated { path: path.clone() }),
        Command::RenameEntry { from, to, .. } => {
            crate::files::rename(scope, from, to).map(|()| Output::EntryRenamed {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::MoveEntryTo { from, to, .. } => {
            crate::files::rename::between(source, scope, from, to).map(|()| Output::EntryMoved {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::TrashEntry { path, .. } => {
            crate::files::trash(scope, path).map(|()| Output::EntryTrashed { path: path.clone() })
        }
        Command::DeleteFile {
            path,
            expected_revision,
            ..
        } => crate::files::mutations::delete(scope, path, expected_revision)
            .map(|()| Output::EntryTrashed { path: path.clone() }),
        Command::MoveFile {
            from,
            to,
            expected_revision,
            ..
        } => crate::files::mutations::move_file(scope, from, to, expected_revision).map(|()| {
            Output::EntryRenamed {
                from: from.clone(),
                to: to.clone(),
            }
        }),
        Command::TrashFile {
            path,
            expected_revision,
            expected_stamp,
            ..
        } => crate::files::remove_verified(scope, path, expected_revision, expected_stamp)
            .map(|()| Output::EntryTrashed { path: path.clone() }),
        Command::CopyEntry { from, to, .. } => {
            crate::files::copy::entry(scope, from, to).map(|()| Output::EntryCopied {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::CopyEntryTo { from, to, .. } => {
            crate::files::copy::between(source, scope, from, to).map(|()| Output::EntryCopied {
                from: from.clone(),
                to: to.clone(),
            })
        }
        Command::WriteFile {
            path,
            text,
            expected_revision,
            ..
        } => crate::files::save(scope, path, text, expected_revision.as_deref())
            .map(Output::FileWritten),
        Command::EditFile {
            path,
            old_text,
            new_text,
            replace_all,
            edits,
            expected_revision,
            ..
        } => {
            let changes = crate::files::edit::changes(
                old_text.as_deref(),
                new_text.as_deref(),
                *replace_all,
                edits.as_deref(),
            )?;
            crate::files::edit(scope, path, &changes, expected_revision).map(Output::FileWritten)
        }
        Command::EditNotebook { edit, .. } => {
            crate::files::notebook::edit(scope, edit).map(Output::FileWritten)
        }
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
            &roots.environment,
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
            &roots.environment,
            message,
            *amend,
            options,
            crate::git::commit::Expected {
                index: expected_index,
                head: expected_head.as_deref(),
                branch: expected_branch.as_deref(),
            },
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
