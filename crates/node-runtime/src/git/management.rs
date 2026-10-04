//! Repository management invoked through the durable worktree command owner.
use super::{
    actions::{name, remote_name},
    cli, git_error,
};
use git2::Repository;
use sailry_protocol::{ErrorCode, Fault, GitAction, GitTag};
use std::path::Path;

pub(super) fn execute(root: &Path, repo: &Repository, action: &GitAction) -> Result<(), Fault> {
    let args: Vec<String> = match action {
        GitAction::DiscardAll { paths } => return discard(root, repo, paths),
        GitAction::RemoveRemote { name: remote } => {
            remote_name(repo, remote)?;
            vec!["remote".into(), "remove".into(), remote.clone()]
        }
        GitAction::CreateTag {
            name: tag,
            commit,
            message,
        } => {
            tag_name(tag)?;
            let oid = git2::Oid::from_str(commit).map_err(git_error)?;
            repo.find_commit(oid).map_err(git_error)?;
            if repo.find_reference(&format!("refs/tags/{tag}")).is_ok() {
                return Err(Fault::new(ErrorCode::Conflict, "tag already exists"));
            }
            let mut args = vec!["tag".into()];
            if let Some(message) = message.as_ref().filter(|m| !m.is_empty()) {
                if message.len() > 16384 || message.contains('\0') {
                    return Err(Fault::new(ErrorCode::InvalidRequest, "invalid tag message"));
                }
                args.extend(["--annotate".into(), "--message".into(), message.clone()]);
            }
            args.extend(["--".into(), tag.clone(), oid.to_string()]);
            args
        }
        GitAction::DeleteTag {
            name: tag,
            commit,
            remote,
        } => {
            tag_name(tag)?;
            let reference = format!("refs/tags/{tag}");
            let oid = git2::Oid::from_str(commit).map_err(git_error)?;
            if let Some(remote) = remote {
                remote_name(repo, remote)?;
                vec![
                    "push".into(),
                    format!("--force-with-lease={reference}:{oid}"),
                    remote.clone(),
                    format!(":{reference}"),
                ]
            } else {
                // Compare-and-delete protects a tag changed since the picker was opened.
                vec!["update-ref".into(), "-d".into(), reference, oid.to_string()]
            }
        }
        GitAction::DeleteRemoteBranch { branch, commit } => {
            let (remote, branch) = remote_branch(repo, branch)?;
            let oid = git2::Oid::from_str(commit).map_err(git_error)?;
            let reference = format!("refs/heads/{branch}");
            vec![
                "push".into(),
                format!("--force-with-lease={reference}:{oid}"),
                remote,
                format!(":{reference}"),
            ]
        }
        GitAction::PushTags { remote } => {
            remote_name(repo, remote)?;
            vec!["push".into(), "--tags".into(), remote.clone()]
        }
        GitAction::DropAllStashes { commits } => {
            let mut repository = Repository::open(root).map_err(git_error)?;
            let mut current = Vec::new();
            repository
                .stash_foreach(|_, _, id| {
                    current.push(id.to_string());
                    true
                })
                .map_err(git_error)?;
            if &current != commits {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "stashes changed before removal",
                ));
            }
            vec!["stash".into(), "clear".into()]
        }
        GitAction::SwitchTracking {
            remote,
            local,
            stash,
        } => {
            remote_branch(repo, remote)?;
            repo.find_branch(remote, git2::BranchType::Remote)
                .map_err(git_error)?;
            name(local)?;
            if !git2::Branch::name_is_valid(local).map_err(git_error)? {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid local branch name",
                ));
            }
            let args = if let Ok(branch) = repo.find_branch(local, git2::BranchType::Local) {
                if branch
                    .upstream()
                    .ok()
                    .and_then(|b| b.name().ok().flatten().map(str::to_owned))
                    .as_deref()
                    != Some(remote)
                {
                    return Err(Fault::new(
                        ErrorCode::Conflict,
                        "local branch tracks a different remote; choose another name",
                    ));
                }
                vec!["switch".into(), local.clone()]
            } else {
                vec![
                    "switch".into(),
                    "--track".into(),
                    "-c".into(),
                    local.clone(),
                    remote.clone(),
                ]
            };
            if *stash {
                stash_before_switch(root)?;
            }
            args
        }
        GitAction::SwitchStashing { branch, commit } => {
            name(branch)?;
            let found = repo
                .find_branch(branch, git2::BranchType::Local)
                .map_err(git_error)?;
            if found.get().target().map(|id| id.to_string()).as_deref() != Some(commit) {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "branch changed before switching",
                ));
            }
            stash_before_switch(root)?;
            vec!["switch".into(), branch.clone()]
        }
        _ => {
            return Err(Fault::new(
                ErrorCode::Internal,
                "unsupported Git management action",
            ));
        }
    };
    cli::run(root, &args).map(|_| ()).map_err(|error| {
        if matches!(
            action,
            GitAction::SwitchStashing { .. } | GitAction::SwitchTracking { stash: true, .. }
        ) {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                format!(
                    "changes were stashed before switching failed: {}",
                    error.message
                ),
            )
        } else {
            error
        }
    })
}

fn stash_before_switch(root: &Path) -> Result<(), Fault> {
    cli::run(
        root,
        &["stash".into(), "push".into(), "--include-untracked".into()],
    )
    .map(|_| ())
}

pub(super) fn remote_branch(repo: &Repository, branch: &str) -> Result<(String, String), Fault> {
    name(branch)?;
    repo.remotes()
        .map_err(git_error)?
        .iter()
        .flatten()
        .flatten()
        .filter_map(|remote| {
            branch
                .strip_prefix(&format!("{remote}/"))
                .map(|name| (remote.to_owned(), name.to_owned()))
        })
        .max_by_key(|(remote, _)| remote.len())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "remote branch has no configured remote",
            )
        })
}

fn tag_name(tag: &str) -> Result<(), Fault> {
    name(tag)?;
    if !git2::Reference::is_valid_name(&format!("refs/tags/{tag}")) {
        return Err(Fault::new(ErrorCode::InvalidRequest, "invalid tag name"));
    }
    Ok(())
}

pub(super) fn remote_tags(root: &Path, remote: &str) -> Result<Vec<GitTag>, Fault> {
    let repo = Repository::open(root).map_err(git_error)?;
    remote_name(&repo, remote)?;
    let output = cli::run(
        root,
        &[
            "ls-remote".into(),
            "--tags".into(),
            "--refs".into(),
            remote.into(),
        ],
    )?;
    let mut result = Vec::new();
    for line in output.lines() {
        if let Some((commit, name)) = line.split_once('\t')
            && let Some(name) = name.strip_prefix("refs/tags/")
        {
            git2::Oid::from_str(commit).map_err(git_error)?;
            result.push(GitTag {
                name: name.into(),
                commit: commit.into(),
            });
            if result.len() > sailry_protocol::MAX_GIT_ENTRIES {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "remote tag listing exceeds limit",
                ));
            }
        }
    }
    Ok(result)
}

pub(super) fn sync(root: &Path, rebase: bool) -> Result<(), Fault> {
    cli::run(
        root,
        &[
            "pull".into(),
            if rebase { "--rebase" } else { "--no-rebase" }.into(),
            "--no-edit".into(),
        ],
    )?;
    cli::run(root, &["push".into()]).map(|_| ())
}

fn discard(root: &Path, repo: &Repository, paths: &[String]) -> Result<(), Fault> {
    if paths.is_empty() || paths.len() > sailry_protocol::MAX_GIT_ENTRIES {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid discard paths",
        ));
    }
    let tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());
    let mut tracked = Vec::new();
    let mut added = Vec::new();
    for path in paths {
        super::valid_path(path)?;
        super::check_entry(root, path)?;
        if tree
            .as_ref()
            .is_some_and(|tree| tree.get_path(Path::new(path)).is_ok())
        {
            tracked.push(path);
        } else {
            added.push(path);
        }
    }
    let mut changed = false;
    let result = (|| {
        if !tracked.is_empty() {
            let mut args = vec![
                "restore".into(),
                "--source=HEAD".into(),
                "--staged".into(),
                "--worktree".into(),
                "--".into(),
            ];
            args.extend(tracked.iter().map(|p| format!(":(literal){p}")));
            cli::run(root, &args)?;
            changed = true;
        }
        for path in added {
            cli::run(
                root,
                &[
                    "rm".into(),
                    "--cached".into(),
                    "--force".into(),
                    "--ignore-unmatch".into(),
                    "--".into(),
                    format!(":(literal){path}"),
                ],
            )?;
            changed = true;
            if root.join(path).symlink_metadata().is_ok() {
                crate::files::trash(root, path)?;
            }
        }
        Ok(())
    })();
    result.map_err(|error: Fault| {
        if changed {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                format!(
                    "some changes were discarded before the operation failed: {}",
                    error.message
                ),
            )
        } else {
            error
        }
    })
}
