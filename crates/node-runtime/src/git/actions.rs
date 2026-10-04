//! Git panel operations use the existing durable worktree mutation boundary.
use super::{cli, git_error, open, revision, valid_path};
use sailry_protocol::{ErrorCode, Fault, GitAction};
use std::path::Path;

pub(super) fn check(
    root: &Path,
    index: &str,
    head: Option<&str>,
    branch: Option<&str>,
) -> Result<(), Fault> {
    let repository = open(root)?;
    let reference = branch
        .map(|name| format!("refs/heads/{name}"))
        .unwrap_or_else(|| "HEAD".into());
    revision::check(
        &repository,
        &reference,
        head.map(git2::Oid::from_str)
            .transpose()
            .map_err(git_error)?,
        index,
    )
}

pub(crate) fn execute(
    root: &Path,
    action: &GitAction,
    index: &str,
    head: Option<&str>,
    branch: Option<&str>,
) -> Result<(), Fault> {
    if matches!(action, GitAction::Initialize) {
        if !index.is_empty() || head.is_some() || branch.is_some() {
            return Err(revision::conflict());
        }
        let directory = super::path::root(root)?;
        match directory.symlink_metadata(".git") {
            Ok(_) => return Err(revision::conflict()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(super::io_error(error)),
        }
        git2::Repository::init_opts(
            root,
            git2::RepositoryInitOptions::new()
                .no_reinit(true)
                .mkdir(false)
                .mkpath(false)
                .external_template(false),
        )
        .map_err(git_error)?;
        return super::check_root(&directory, root);
    }
    check(root, index, head, branch)?;
    let repository = git2::Repository::open(root).map_err(git_error)?;
    let mut args: Vec<String> = Vec::new();
    match action {
        GitAction::Initialize => unreachable!("initialization is handled before revision checks"),
        GitAction::PruneWorktree { path } => {
            return super::worktrees::prune(&repository, Path::new(path));
        }
        GitAction::Ignore { paths, directories } => {
            return super::ignore::add(root, &repository, paths, *directories);
        }
        GitAction::StartBranch { name } => {
            if head.is_some() {
                return Err(revision::conflict());
            }
            match repository.head() {
                Err(error) if error.code() == git2::ErrorCode::UnbornBranch => {}
                Err(error) => return Err(git_error(error)),
                Ok(_) => return Err(revision::conflict()),
            }
            if name.starts_with('-')
                || name.len() > 1024
                || !git2::Branch::name_is_valid(name).map_err(git_error)?
            {
                return Err(Fault::new(ErrorCode::InvalidRequest, "invalid branch name"));
            }
            if branch == Some(name.as_str())
                || repository
                    .find_branch(name, git2::BranchType::Local)
                    .is_ok()
            {
                return Err(Fault::new(ErrorCode::Conflict, "branch already exists"));
            }
            args.extend(["checkout".into(), "-b".into(), name.clone()]);
        }
        GitAction::AddRemote { name: remote, url } => {
            name(remote)?;
            if url.is_empty()
                || url.len() > 4096
                || url.starts_with('-')
                || url.chars().any(char::is_control)
            {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid Git remote URL",
                ));
            }
            args.extend([
                "remote".into(),
                "add".into(),
                "--".into(),
                remote.clone(),
                url.clone(),
            ]);
        }

        GitAction::Fetch { remote, prune, all } => {
            args.push("fetch".into());
            if *prune {
                args.push("--prune".into());
            }
            if *all && remote.is_some() {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "choose one remote or all remotes",
                ));
            }
            if let Some(remote) = remote {
                remote_name(&repository, remote)?;
                args.push(remote.clone());
            } else if *all {
                args.push("--all".into());
            }
        }
        GitAction::Pull {
            rebase,
            remote,
            branch: source,
        } => {
            args.extend([
                "pull".into(),
                if *rebase { "--rebase" } else { "--no-rebase" }.into(),
                "--no-edit".into(),
            ]);
            if let Some(remote) = remote {
                remote_name(&repository, remote)?;
                args.push(remote.clone());
                if let Some(source) = source {
                    name(source)?;
                    args.push(source.clone());
                }
            } else if source.is_some() {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "pull branch requires a remote",
                ));
            }
        }
        GitAction::Sync { rebase } => return super::management::sync(root, *rebase),
        GitAction::DiscardAll { .. }
        | GitAction::RemoveRemote { .. }
        | GitAction::DeleteRemoteBranch { .. }
        | GitAction::CreateTag { .. }
        | GitAction::DeleteTag { .. }
        | GitAction::PushTags { .. }
        | GitAction::DropAllStashes { .. }
        | GitAction::SwitchTracking { .. }
        | GitAction::SwitchStashing { .. } => {
            return super::management::execute(root, &repository, action);
        }
        GitAction::Push {
            remote,
            publish,
            force,
        } => {
            args.push("push".into());
            if *publish {
                args.push("--set-upstream".into());
            }
            if *force {
                args.push("--force-with-lease".into());
            }
            if let Some(remote) = remote {
                remote_name(&repository, remote)?;
                args.push(remote.clone());
                let branch = branch.ok_or_else(|| {
                    Fault::new(
                        ErrorCode::InvalidRequest,
                        "push requires an attached branch",
                    )
                })?;
                args.push(format!("refs/heads/{branch}:refs/heads/{branch}"));
            } else if *publish {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "select a remote to publish",
                ));
            }
        }
        GitAction::Merge { commit }
        | GitAction::Rebase { commit }
        | GitAction::CherryPick { commit } => {
            let oid = git2::Oid::from_str(commit).map_err(git_error)?;
            repository.find_commit(oid).map_err(git_error)?;
            args.extend([
                if matches!(action, GitAction::Merge { .. }) {
                    "merge"
                } else if matches!(action, GitAction::Rebase { .. }) {
                    "rebase"
                } else {
                    "cherry-pick"
                }
                .into(),
                oid.to_string(),
            ]);
        }
        GitAction::Track { branch: target } | GitAction::SwitchRemote { branch: target } => {
            name(target)?;
            repository
                .find_branch(target, git2::BranchType::Remote)
                .map_err(git_error)?;
            if matches!(action, GitAction::Track { .. }) {
                args.extend(["branch".into(), format!("--set-upstream-to={target}")]);
            } else {
                let remotes = repository.remotes().map_err(git_error)?;
                let existing = remotes
                    .iter()
                    .flatten()
                    .flatten()
                    .filter_map(|remote| target.strip_prefix(&format!("{remote}/")))
                    .filter(|name| {
                        repository
                            .find_branch(name, git2::BranchType::Local)
                            .is_ok()
                    })
                    .min_by_key(|name| name.len());
                if let Some(local) = existing {
                    let local_branch = repository
                        .find_branch(local, git2::BranchType::Local)
                        .map_err(git_error)?;
                    if local_branch
                        .upstream()
                        .ok()
                        .and_then(|b| b.name().ok().flatten().map(str::to_owned))
                        .as_deref()
                        != Some(target)
                    {
                        return Err(Fault::new(
                            ErrorCode::Conflict,
                            "local branch tracks a different remote",
                        ));
                    }
                    args.extend(["switch".into(), local.into()]);
                } else {
                    args.extend(["switch".into(), "--track".into(), target.clone()]);
                }
            }
        }
        GitAction::Stash { mode } => {
            args.extend(["stash".into(), "push".into()]);
            match mode {
                sailry_protocol::GitStashMode::All => args.push("--include-untracked".into()),
                sailry_protocol::GitStashMode::Staged => args.push("--staged".into()),
                _ => {}
            }
        }
        GitAction::ApplyStash { commit, pop } => {
            let position = stash_position(root, commit)?;
            cli::run(root, &["stash".into(), "apply".into(), commit.clone()])?;
            if *pop {
                if stash_position(root, commit)? != position {
                    return Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "stash applied but its position changed before removal",
                    ));
                }
                cli::run(
                    root,
                    &[
                        "stash".into(),
                        "drop".into(),
                        format!("stash@{{{position}}}"),
                    ],
                )
                .map_err(|_| {
                    Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "stash applied but removal could not be confirmed",
                    )
                })?;
            }
            return Ok(());
        }
        GitAction::DropStash { commit } => {
            let position = stash_position(root, commit)?;
            args.extend([
                "stash".into(),
                "drop".into(),
                format!("stash@{{{position}}}"),
            ]);
        }
        GitAction::UndoCommit => {
            let commit = repository
                .head()
                .map_err(git_error)?
                .peel_to_commit()
                .map_err(git_error)?;
            if commit.parent_count() > 0 {
                let parent = commit.parent_id(0).map_err(git_error)?;
                args.extend(["reset".into(), "--soft".into(), parent.to_string()]);
            } else if repository.head().map_err(git_error)?.is_branch() {
                // Removing the first branch commit leaves its files staged in an unborn branch.
                args.extend([
                    "update-ref".into(),
                    "-d".into(),
                    "HEAD".into(),
                    commit.id().to_string(),
                ]);
            } else {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "cannot undo a detached root commit",
                ));
            }
        }
        GitAction::Discard { paths } => {
            if paths.is_empty() || paths.len() > sailry_protocol::MAX_GIT_ENTRIES {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid discard paths",
                ));
            }
            // Discard tracked edits only. Untracked files use the existing trash action.
            let tree = repository
                .head()
                .map_err(git_error)?
                .peel_to_tree()
                .map_err(git_error)?;
            for path in paths {
                valid_path(path)?;
                super::check_entry(root, path)?;
                if tree.get_path(Path::new(path)).is_err() {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "discard requires tracked files",
                    ));
                }
            }
            args.extend([
                "restore".into(),
                "--source=HEAD".into(),
                "--staged".into(),
                "--worktree".into(),
                "--".into(),
            ]);
            // Literal pathspecs belong only to explicit file arguments. The global flag
            // breaks Git stash's internal untracked-file cleanup.
            args.extend(paths.iter().map(|path| format!(":(literal){path}")));
        }
        GitAction::Trash { paths } => {
            if paths.is_empty() || paths.len() > sailry_protocol::MAX_GIT_ENTRIES {
                return Err(Fault::new(ErrorCode::InvalidRequest, "invalid trash paths"));
            }
            for path in paths {
                valid_path(path)?;
                super::check_entry(root, path)?;
                if !repository
                    .status_file(Path::new(path))
                    .map_err(git_error)?
                    .is_wt_new()
                {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "trash target is no longer untracked",
                    ));
                }
            }
            for (position, path) in paths.iter().enumerate() {
                crate::files::trash(root, path).map_err(|error| {
                    if position == 0 {
                        error
                    } else {
                        Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "some entries moved to Trash; inspect repository before retrying",
                        )
                    }
                })?;
            }
            return Ok(());
        }
        GitAction::Continue | GitAction::Abort => {
            let operation = match repository.state() {
                git2::RepositoryState::Rebase
                | git2::RepositoryState::RebaseInteractive
                | git2::RepositoryState::RebaseMerge => "rebase",
                git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
                    "cherry-pick"
                }
                git2::RepositoryState::Merge => "merge",
                _ => {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "no Git operation to continue or abort",
                    ));
                }
            };
            args.extend([
                operation.into(),
                if matches!(action, GitAction::Continue) {
                    "--continue"
                } else {
                    "--abort"
                }
                .into(),
            ]);
        }
    }
    cli::run(root, &args).map(|_| ())
}

pub(super) fn name(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.starts_with('-')
        || value.len() > 1024
        || value.chars().any(char::is_control)
    {
        return Err(Fault::new(ErrorCode::InvalidRequest, "invalid Git target"));
    }
    Ok(())
}
pub(super) fn remote_name(repository: &git2::Repository, value: &str) -> Result<(), Fault> {
    name(value)?;
    repository.find_remote(value).map_err(git_error)?;
    Ok(())
}

pub(super) fn stash_position(root: &Path, commit: &str) -> Result<usize, Fault> {
    let id = git2::Oid::from_str(commit).map_err(git_error)?;
    let mut repository = git2::Repository::open(root).map_err(git_error)?;
    let mut found = None;
    repository
        .stash_foreach(|position, _, target| {
            if *target == id {
                found = Some(position);
                false
            } else {
                true
            }
        })
        .map_err(git_error)?;
    found.ok_or_else(|| {
        Fault::new(
            ErrorCode::RevisionConflict,
            "selected stash no longer exists",
        )
    })
}
