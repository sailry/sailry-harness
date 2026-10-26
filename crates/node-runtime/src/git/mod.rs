//! Read services adapted from sailry-code 67ae9fa0,
//! sailry-git/src/action_read/{repository,status,diff}.rs (Apache-2.0).
//! libgit2 stays on bounded Node workers, without SSH/HTTPS or UI dependencies.
pub(crate) mod actions;
pub(crate) mod branches;
mod changes;
pub(crate) mod checkout;
mod cli;
pub(crate) mod commit;
pub(crate) mod credentials;
mod diff;
mod head;
mod history;
mod ignore;
pub(crate) mod index;
mod inspection;
mod management;
pub(crate) mod merge;
mod output;
mod revision;
mod scans;
mod status;
mod watch;
pub(crate) mod worktrees;

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cap_std::fs::Dir;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared, WeakShared},
};
use git2::{Config, Repository, RepositoryOpenFlags};
use sailry_link::CancellationToken;
use sailry_protocol::*;

use crate::files::path;
pub(crate) use watch::WatchFilter;

type StatusRead = BoxFuture<'static, Result<Output, Fault>>;
const STATUS_TTL: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct CachedStatus {
    output: Output,
    root: path::identity::Identity,
    expires: tokio::time::Instant,
}

enum StatusSlot {
    Pending {
        read: WeakShared<StatusRead>,
        generation: Arc<()>,
    },
    Complete(Box<CachedStatus>),
}

#[derive(Clone)]
pub(crate) struct Git {
    output: output::Journal,
    environment: Arc<crate::process::Environment>,
    statuses: Arc<Mutex<BTreeMap<PathBuf, StatusSlot>>>,
    changes: Arc<changes::Cache>,
    scans: Arc<scans::Scans>,
}

impl Git {
    pub(crate) fn new(environment: Arc<crate::process::Environment>) -> Self {
        Self {
            output: Default::default(),
            environment,
            statuses: Default::default(),
            changes: Default::default(),
            scans: Default::default(),
        }
    }

    pub(crate) fn watch_roots(root: &Path) -> Result<Vec<PathBuf>, Fault> {
        let repository = match open(root) {
            Ok(repository) => repository,
            Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let mut roots = vec![
            repository.path().to_path_buf(),
            repository.commondir().to_path_buf(),
        ];
        roots.sort_unstable();
        roots.dedup();
        for root in &roots {
            path::root(root)?;
        }
        Ok(roots)
    }

    pub(crate) fn watch_filter(root: &Path) -> Result<WatchFilter, Fault> {
        WatchFilter::new(root)
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
        if matches!(&command, Command::InspectGit { .. }) {
            if closed.is_cancelled() {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "Node service is unavailable",
                ));
            }
            let read = self.status(root, command);
            return tokio::select! {
                biased;
                _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
                result = read => result,
            };
        }
        self.inspect_once(root, command, closed, None)
            .await
            .map(|(output, _)| output)
    }

    fn status(&self, root: PathBuf, command: Command) -> Shared<StatusRead> {
        let mut statuses = self
            .statuses
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = tokio::time::Instant::now();
        statuses.retain(|_, slot| match slot {
            StatusSlot::Pending { read, .. } => read.upgrade().is_some(),
            StatusSlot::Complete(cached) => now < cached.expires,
        });
        let cached = match statuses.get(&root) {
            Some(StatusSlot::Pending { read, .. }) => {
                if let Some(read) = read.upgrade() {
                    return read;
                }
                None
            }
            Some(StatusSlot::Complete(cached)) => Some(cached.as_ref().clone()),
            None => None,
        };
        let git = self.clone();
        let source = root.clone();
        let generation = Arc::new(());
        let current = generation.clone();
        let read = async move {
            let expires = cached.as_ref().map(|cached| cached.expires);
            // Pending slots stay weak: the last observer still cancels its worker.
            let result = git
                .inspect_once(source.clone(), command, CancellationToken::new(), cached)
                .await;
            let mut statuses = git
                .statuses
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            // A write may have replaced this slot while its scan was running.
            if matches!(statuses.get(&source), Some(StatusSlot::Pending { generation, .. })
                if Arc::ptr_eq(generation, &current))
            {
                match &result {
                    Ok((output, root)) => {
                        statuses.insert(
                            source,
                            StatusSlot::Complete(Box::new(CachedStatus {
                                output: output.clone(),
                                root: *root,
                                expires: expires
                                    .unwrap_or_else(|| tokio::time::Instant::now() + STATUS_TTL),
                            })),
                        );
                    }
                    Err(_) => {
                        statuses.remove(&source);
                    }
                }
            }
            result.map(|(output, _)| output)
        }
        .boxed()
        .shared();
        statuses.insert(
            root,
            StatusSlot::Pending {
                read: read.downgrade().expect("new status read is pending"),
                generation,
            },
        );
        read
    }

    pub(crate) fn invalidate(&self, root: &Path) {
        self.statuses
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(root);
    }

    async fn inspect_once(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
        cached: Option<CachedStatus>,
    ) -> Result<(Output, path::identity::Identity), Fault> {
        let output = self.output.clone();
        let environment = self.environment.clone();
        let captures = self.changes.clone();
        let deadline = if matches!(command, Command::ListGitRemoteTags { .. }) {
            Duration::from_secs(125)
        } else {
            Duration::from_secs(5)
        };
        let expires = Instant::now() + deadline;
        let permit = if matches!(
            &command,
            Command::InspectGit { .. }
                | Command::ListGitChanges { .. }
                | Command::ReadGitChange { .. }
                | Command::ReadGitContext { .. }
        ) {
            let gate = self.scans.root(&root);
            Some(tokio::select! {
                biased;
                _ = closed.cancelled() => return Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
                permit = tokio::time::timeout_at(expires.into(), gate.acquire_owned()) =>
                    permit.map_err(|_| Fault::new(ErrorCode::Busy, "Git read deadline exceeded"))?
                        .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git inspection is unavailable"))?,
            })
        } else {
            None
        };
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            // A timed-out observer cannot release an executing libgit2 scan.
            let _permit = permit;
            let control = Control {
                cancelled,
                deadline: expires,
            };
            control.check()?;
            let retained = path::root(&root)?;
            let identity = path::identity::directory(&retained)?;
            if let Some(cached) = cached {
                if identity != cached.root {
                    return Err(Fault::new(
                        ErrorCode::Conflict,
                        "worktree root changed since Git inspection",
                    ));
                }
                match open(&root) {
                    Ok(_) => {}
                    Err(error)
                        if error.code == ErrorCode::NotFound
                            && matches!(&cached.output, Output::GitStatus(status)
                                if status.kind == RepositoryKind::Directory)
                            && !root.join(".git").exists() => {}
                    Err(error) => return Err(error),
                }
                check_root(&retained, &root)?;
                control.check()?;
                return Ok((cached.output, identity));
            }
            let result = match command {
                Command::ReadGitStash { commit, .. } => {
                    actions::stash_position(&root, &commit)?;
                    cli::run(
                        &root,
                        &environment,
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
                    .capture(&root, || {
                        management::remote_tags(&root, &environment, &remote)
                    })
                    .map(Output::GitRemoteTags),
                Command::InspectGit { .. } => {
                    status::inspect(&root, &control).map(Output::GitStatus)
                }
                Command::ReadGitHead { .. } => head::read(&root).map(Output::GitHead),
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
                Command::ListGitChanges {
                    target,
                    cursor,
                    limit,
                    ..
                } => changes::list(&root, target, cursor, limit, &control, &captures)
                    .map(Output::GitChanges),
                Command::ReadGitChange {
                    snapshot,
                    path,
                    expected_metadata_revision,
                    expected_revision,
                    offset,
                    limit,
                    ..
                } => changes::read(
                    &root,
                    snapshot,
                    &path,
                    &expected_metadata_revision,
                    changes::Page {
                        expected_revision: expected_revision.as_deref(),
                        offset,
                        limit,
                    },
                    &control,
                    &captures,
                )
                .map(Output::GitChange),
                Command::ReadGitContext {
                    snapshot,
                    path,
                    expected_metadata_revision,
                    expected_revision,
                    side,
                    offset,
                    limit,
                    ..
                } => changes::context(
                    &root,
                    snapshot,
                    &path,
                    &expected_metadata_revision,
                    (
                        side,
                        changes::Page {
                            expected_revision: Some(&expected_revision),
                            offset,
                            limit,
                        },
                    ),
                    &control,
                    &captures,
                )
                .map(Output::GitContext),
                Command::ReadGitShow {
                    revision,
                    path,
                    offset,
                    limit,
                    ..
                } => inspection::show(&root, &revision, path.as_deref(), offset, limit, &control),
                Command::ReadGitBlame {
                    revision,
                    path,
                    offset,
                    limit,
                    ..
                } => inspection::blame(&root, revision.as_deref(), &path, offset, limit, &control)
                    .map(Output::GitBlame),
                _ => Err(Fault::new(
                    ErrorCode::Internal,
                    "invalid Git inspection command",
                )),
            };
            check_root(&retained, &root)?;
            control.check()?;
            result.map(|output| (output, identity))
        });
        // Use the runtime blocking pool. Queued work checks cancellation before I/O;
        // an executing libgit2 call still cannot be preempted.
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
            result = tokio::time::timeout_at(expires.into(), worker) => result
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
            return Err(Fault::invalid("Git metadata cannot be a symlink"));
        }
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => {}
        Ok(_) => {
            return Err(Fault::invalid("unsupported Git metadata entry"));
        }
        Err(error) => return Err(io_error(error)),
    }
    let repository = Repository::open_ext(
        root,
        RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&OsStr>(),
    )
    .map_err(fault)?;
    let workdir = repository
        .workdir()
        .ok_or_else(|| Fault::invalid("expected a non-bare worktree"))?;
    if workdir.canonicalize().map_err(io_error)? != root {
        return Err(Fault::invalid(
            "expected the exact registered worktree root",
        ));
    }
    // Do not inherit command-valued filters, fsmonitor, credential or transport configuration.
    repository
        .set_config(&Config::new().map_err(fault)?)
        .map_err(fault)?;
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

pub(crate) fn valid_path(value: &str) -> Result<(), Fault> {
    let parts = path::components(value, false)?;
    if value.chars().any(char::is_control)
        || parts.iter().any(|part| part.eq_ignore_ascii_case(".git"))
    {
        return Err(Fault::invalid("invalid Git worktree-relative path"));
    }
    Ok(())
}

fn check_entry(root: &Path, value: &str) -> Result<(), Fault> {
    check_entry_kind(root, value, false)
}

fn check_entry_kind(root: &Path, value: &str, gitlink: bool) -> Result<(), Fault> {
    EntryParents::new(path::root(root)?).check(value, gitlink)
}

struct EntryParents {
    stack: Vec<(String, Option<Dir>)>,
}

impl EntryParents {
    fn new(root: Dir) -> Self {
        Self {
            stack: vec![(String::new(), Some(root))],
        }
    }

    fn check(&mut self, value: &str, gitlink: bool) -> Result<(), Fault> {
        use cap_fs_ext::DirExt;
        let parts = path::components(value, false)?;
        let parents = &parts[..parts.len() - 1];
        let shared = parents
            .iter()
            .zip(self.stack.iter().skip(1))
            .take_while(|(name, (retained, _))| **name == retained.as_str())
            .count();
        self.stack.truncate(shared + 1);
        for name in &parents[shared..] {
            let dir = match &self.stack.last().expect("root is retained").1 {
                Some(parent) => match parent.open_dir_nofollow(name) {
                    Ok(dir) => {
                        #[cfg(windows)]
                        path::identity::directory(&dir)?;
                        Some(dir)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(io_error(error)),
                },
                None => None,
            };
            self.stack.push(((*name).to_owned(), dir));
        }
        let Some(dir) = &self.stack.last().expect("root is retained").1 else {
            return Ok(());
        };
        match dir.symlink_metadata(parts[parts.len() - 1]) {
            Ok(metadata)
                if metadata.is_file() || metadata.is_symlink() || gitlink && metadata.is_dir() =>
            {
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
            _ => Err(Fault::invalid(
                "Git file inspection requires a regular file or symlink",
            )),
        }
    }
}

fn fault(error: git2::Error) -> Fault {
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
