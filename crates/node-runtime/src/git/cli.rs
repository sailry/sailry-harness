//! Native Git preserves the execution Node's credentials, hooks and signing configuration.
//! Arguments are passed directly; no shell or controller-side Git process is involved.
use super::{check_root, path};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use sailry_protocol::{ErrorCode, Fault};
use std::{path::Path, process::Stdio, time::Duration};

pub(super) fn run(root: &Path, args: &[String]) -> Result<String, Fault> {
    let retained = path::root(root)?;
    super::output::command(args);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git process runtime unavailable"))?;
    let result = runtime.block_on(async {
        let mut command = tokio::process::Command::new("git");
        // Ambient repository overrides must not redirect the authenticated worktree.
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .current_dir(root)
            .arg("--no-pager")
            .args(args)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .env("GIT_SEQUENCE_EDITOR", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        let mut child = command.spawn().map_err(|_| {
            Fault::new(
                ErrorCode::Unavailable,
                "Git executable unavailable on execution Node",
            )
        })?;
        let mut drain = crate::process::output::Drain::new(
            child.stdout().take().unwrap(),
            child.stderr().take().unwrap(),
        );
        let result = tokio::time::timeout(Duration::from_secs(120), child.wait()).await;
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        let (stdout, stderr, error) = drain.finish(Duration::from_secs(2)).await;
        let status = result
            .map_err(|_| {
                Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "Git operation timed out; inspect repository before retrying",
                )
            })?
            .map_err(|_| Fault::new(ErrorCode::OutcomeUnknown, "Git process result unavailable"))?;
        super::output::record(&format!(
            "{}{}\n[exit {}]\n",
            stdout.text,
            stderr.text,
            status.code().unwrap_or(-1)
        ));
        if error.is_some() {
            return Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "Git output could not be read",
            ));
        }
        if !status.success() {
            let message = stderr.text.to_ascii_lowercase();
            let code = if message.contains(".lock") {
                ErrorCode::Busy
            } else if message.contains("gpg") || message.contains("signing failed") {
                ErrorCode::Unavailable
            } else if message.contains("empty ident") {
                ErrorCode::NotConfigured
            } else if message.contains("conflict") || message.contains("would be overwritten") {
                ErrorCode::Conflict
            } else if message.contains("authentication") || message.contains("permission denied") {
                ErrorCode::PermissionDenied
            } else if message.contains("user.email") || message.contains("user.name") {
                ErrorCode::NotConfigured
            } else {
                ErrorCode::InvalidRequest
            };
            // Remote URLs and hook output may contain credentials. Do not put them in durable receipts.
            return Err(Fault::new(
                code,
                format!(
                    "Git operation failed (exit {})",
                    status.code().unwrap_or(-1)
                ),
            ));
        }
        if stdout.truncated && args.first().is_some_and(|arg| arg == "ls-remote") {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "Git remote listing exceeds output limit",
            ));
        }
        Ok(stdout.text)
    });
    check_root(&retained, root).map_err(|_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "Git worktree changed during operation",
        )
    })?;
    result
}
