//! Commit creation uses native Git so hooks, signoff and signing follow repository settings.
use super::{actions, cli, git_error};
use git2::Repository;
use sailry_protocol::{ErrorCode, Fault, GitCommitOptions};
use std::path::Path;

pub(crate) fn create(
    root: &Path,
    message: &str,
    amend: bool,
    options: &GitCommitOptions,
    expected_index: &str,
    expected_head: Option<&str>,
    expected_branch: Option<&str>,
) -> Result<(String, Option<Fault>), Fault> {
    if message.trim().is_empty() || message.len() > 16 * 1024 || message.contains('\0') {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid Git commit message",
        ));
    }
    actions::check(root, expected_index, expected_head, expected_branch)?;
    let mut args = vec!["commit".into(), "--message".into(), message.into()];
    if amend {
        args.push("--amend".into());
    }
    if options.signoff {
        args.push("--signoff".into());
    }
    if options.skip_hooks {
        args.push("--no-verify".into());
    }
    if options.all {
        cli::run(root, &["add".into(), "--all".into()])?;
    }
    if options.tracked {
        args.push("--all".into());
    }
    cli::run(root, &args)?;
    let id = Repository::open(root)
        .and_then(|repo| {
            repo.head()?
                .peel_to_commit()
                .map(|commit| commit.id().to_string())
        })
        .map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "Git commit result could not be confirmed",
            )
        })?;
    let follow_up = (|| match options.after {
        sailry_protocol::GitAfterCommit::None => Ok(()),
        sailry_protocol::GitAfterCommit::Push | sailry_protocol::GitAfterCommit::Sync
            if options.push_remote.is_some() =>
        {
            let remote = options.push_remote.as_ref().unwrap();
            let repository = Repository::open(root).map_err(git_error)?;
            actions::remote_name(&repository, remote)?;
            let branch = expected_branch.ok_or_else(|| {
                Fault::new(
                    ErrorCode::InvalidRequest,
                    "publishing requires an attached branch",
                )
            })?;
            cli::run(
                root,
                &[
                    "push".into(),
                    "--set-upstream".into(),
                    remote.clone(),
                    format!("HEAD:refs/heads/{branch}"),
                ],
            )
            .map(|_| ())
        }
        sailry_protocol::GitAfterCommit::Push => cli::run(root, &["push".into()]).map(|_| ()),
        sailry_protocol::GitAfterCommit::Sync => super::management::sync(root, false),
    })()
    .err();
    Ok((id, follow_up))
}

pub(super) fn signature(repository: &Repository) -> Result<git2::Signature<'static>, Fault> {
    // Reading identity/config does not execute filters, hooks, or signing programs.
    let configured = Repository::open(repository.path()).map_err(git_error)?;
    let signature = configured.signature().map_err(|_| {
        Fault::new(
            ErrorCode::NotConfigured,
            "configure Git user.name and user.email on the execution Node",
        )
    })?;
    match configured
        .config()
        .map_err(git_error)?
        .get_bool("commit.gpgsign")
    {
        Ok(true) => {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "signed Git commits are not integrated; use Git to preserve the signing configuration",
            ));
        }
        Ok(false) => {}
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(git_error(error)),
    }
    Ok(signature)
}
