//! Read services adapted from sailry-code 67ae9fa0,
//! sailry-git/src/action_read/{repository,status,diff}.rs (Apache-2.0).
//! libgit2 stays on bounded Node workers, without SSH/HTTPS or UI dependencies.
pub(crate) mod actions;
pub(crate) mod branches;
pub(crate) mod checkout;
mod cli;
pub(crate) mod commit;
pub(crate) mod credentials;
mod diff;
mod history;
mod ignore;
pub(crate) mod index;
mod management;
pub(crate) mod merge;
mod output;
mod revision;
mod status;
pub(crate) mod worktrees;

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use cap_std::fs::Dir;
use git2::{Config, Repository, RepositoryOpenFlags};
use sailry_link::CancellationToken;
use sailry_protocol::*;

use crate::files::path;

#[derive(Clone)]
pub(crate) struct Git {
    output: output::Journal,
}

impl Git {
    pub(crate) fn new() -> Self {
        Self {
            output: Default::default(),
        }
    }

    pub(crate) fn record(command: &Command, result: &sailry_link::Response) {
        let label = match command {
            Command::RunGitAction { .. } => "Git action",
            Command::CreateGitCommit { .. } => "Git commit",
            Command::SwitchGitBranch { .. } => "Git checkout",
            Command::CreateGitBranch { .. } => "Git branch creation",
            Command::RenameGitBranch { .. } => "Git branch rename",
            Command::DeleteGitBranch { .. } => "Git branch deletion",
            Command::MergeGitBranch { .. } => "Git merge",
            Command::UpdateGitIndex { .. } => "Git staging",
            _ => return,
        };
        match result {
            Ok(Output::GitCommitCreated {
                follow_up: Some(error),
                ..
            }) => output::record(&format!(
                "Git commit: completed; follow-up failed: {}\n",
                error.message
            )),
            Ok(_) => output::record(&format!("{label}: completed\n")),
            Err(fault) => {
                output::record(&format!("{label}: {:?}: {}\n", fault.code, fault.message))
            }
        }
    }

    pub(crate) fn capture<T>(&self, root: &Path, run: impl FnOnce() -> T) -> T {
        self.output.capture(root, run)
    }

    pub(crate) async fn inspect(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
    ) -> Result<Output, Fault> {
        let output = self.output.clone();
        let deadline = if matches!(command, Command::ListGitRemoteTags { .. }) {
            Duration::from_secs(125)
        } else {
            Duration::from_secs(5)
        };
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            let control = Control {
                cancelled,
                deadline: Instant::now() + deadline,
            };
            control.check()?;
            let retained = path::root(&root)?;
            let result = match command {
                Command::ReadGitStash { commit, .. } => {
                    actions::stash_position(&root, &commit)?;
                    cli::run(
                        &root,
                        &[
                            "stash".into(),
                            "show".into(),
                            "--include-untracked".into(),
                            "--patch".into(),
                            commit,
                        ],
                    )
                    .map(|text| Output::GitOutput { text })
                }
                Command::ReadGitOutput { .. } => Ok(Output::GitOutput {
                    text: output.read(&root),
                }),
                Command::ListGitRemoteTags { remote, .. } => output
                    .capture(&root, || management::remote_tags(&root, &remote))
                    .map(Output::GitRemoteTags),
                Command::InspectGit { .. } => {
                    status::inspect(&root, &control).map(Output::GitStatus)
                }
                Command::ListGitBranches { .. } => {
                    branches::list(&root, &control).map(Output::GitBranches)
                }
                Command::ResolveGitRevision { revision, .. } => {
                    revision::resolve(&root, &revision).map(|commit| Output::GitRevision { commit })
                }
                Command::ListWorktrees { .. } => {
                    worktrees::list(&root, &control).map(Output::GitWorktrees)
                }
                Command::ReadGitDiff { path, scope, .. } => {
                    diff::read(&root, path, scope, &control).map(Output::GitDiff)
                }
                Command::ReadGitLog { limit, cursor, .. } => {
                    history::log(&root, limit, cursor.as_ref(), &control).map(Output::GitLog)
                }
                Command::ReadGitCommit { commit, .. } => {
                    history::commit(&root, &commit, &control).map(Output::GitCommit)
                }
                _ => Err(Fault::new(
                    ErrorCode::Internal,
                    "invalid Git inspection command",
                )),
            };
            check_root(&retained, &root)?;
            control.check()?;
            result
        });
        // Use the runtime blocking pool. Queued work checks cancellation before I/O;
        // an executing libgit2 call still cannot be preempted.
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
            result = tokio::time::timeout(deadline, worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "Git read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "Git worker failed"))?,
        }
    }
}

struct Control {
    cancelled: CancellationToken,
    deadline: Instant,
}

impl Control {
    fn check(&self) -> Result<(), Fault> {
        if self.cancelled.is_cancelled() {
            Err(Fault::new(ErrorCode::Cancelled, "Git read cancelled"))
        } else if Instant::now() >= self.deadline {
            Err(Fault::new(ErrorCode::Busy, "Git read deadline exceeded"))
        } else {
            Ok(())
        }
    }
}

fn open(root: &Path) -> Result<Repository, Fault> {
    let dir = path::root(root)?;
    match dir.symlink_metadata(".git") {
        Ok(metadata) if metadata.is_symlink() => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "Git metadata cannot be a symlink",
            ));
        }
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => {}
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "unsupported Git metadata entry",
            ));
        }
        Err(error) => return Err(io_error(error)),
    }
    let repository = Repository::open_ext(
        root,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&OsStr>(),
    )
    .map_err(git_error)?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "expected a non-bare worktree"))?;
    if workdir.canonicalize().map_err(io_error)? != root {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected the exact registered worktree root",
        ));
    }
    // Do not inherit command-valued filters, fsmonitor, credential or transport configuration.
    repository
        .set_config(&Config::new().map_err(git_error)?)
        .map_err(git_error)?;
    Ok(repository)
}

fn check_root(retained: &Dir, root: &Path) -> Result<(), Fault> {
    let current = path::root(root)?;
    if path::identity::directory(retained)? != path::identity::directory(&current)? {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "worktree root changed during Git operation",
        ));
    }
    Ok(())
}

fn valid_path(value: &str) -> Result<(), Fault> {
    let parts = path::components(value, false)?;
    if value.chars().any(char::is_control)
        || parts.iter().any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid Git worktree-relative path",
        ));
    }
    Ok(())
}

fn check_entry(root: &Path, value: &str) -> Result<(), Fault> {
    use cap_fs_ext::DirExt;
    let parts = path::components(value, false)?;
    let mut dir = path::root(root)?;
    for part in &parts[..parts.len() - 1] {
        dir = match dir.open_dir_nofollow(part) {
            Ok(dir) => dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        };
    }
    match dir.symlink_metadata(parts[parts.len() - 1]) {
        Ok(metadata) if metadata.is_file() || metadata.is_symlink() => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
        _ => Err(Fault::new(
            ErrorCode::InvalidRequest,
            "Git file inspection requires a regular file or symlink",
        )),
    }
}

fn git_error(error: git2::Error) -> Fault {
    let code = match error.code() {
        git2::ErrorCode::NotFound | git2::ErrorCode::UnbornBranch => ErrorCode::NotFound,
        git2::ErrorCode::Locked => ErrorCode::Busy,
        _ => ErrorCode::InvalidRequest,
    };
    Fault::new(code, format!("Git operation failed ({:?})", error.code()))
}

fn io_error(error: std::io::Error) -> Fault {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        _ => ErrorCode::InvalidRequest,
    };
    Fault::new(
        code,
        format!("Git filesystem access failed ({:?})", error.kind()),
    )
}

#[cfg(test)]
mod tests;
