//! GitHub CLI credentials are collected only for an explicit Node import.
use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

fn executable(environment: &Environment) -> Option<PathBuf> {
    which::which_in(
        "gh",
        environment.path(),
        environment.home().unwrap_or(std::path::Path::new("/")),
    )
    .ok()
}

fn command(environment: &Environment, path: &std::path::Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(path);
    environment.apply(&mut command);
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

pub(crate) async fn status(
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<(bool, bool), Fault> {
    let Some(path) = executable(environment) else {
        return Ok((false, false));
    };
    let mut command = command(environment, &path);
    command
        .args(["auth", "status", "--active", "--hostname", "github.com"])
        .stdout(Stdio::null());
    let state = tokio::select! {
        _ = stop.cancelled() => return Err(unavailable()),
        result = tokio::time::timeout(Duration::from_secs(15), command.status()) =>
            result.map_err(|_| unavailable())?.map_err(|_| unavailable())?,
    };
    Ok((true, state.success()))
}

pub(crate) async fn token(
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<Secret, Fault> {
    let path = executable(environment)
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "GitHub CLI is unavailable"))?;
    let mut command = command(environment, &path);
    command
        .args(["auth", "token", "--hostname", "github.com"])
        .stdout(Stdio::piped());
    let operation = async {
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let mut stdout = child.stdout.take().ok_or_else(unavailable)?.take(4097);
        let mut bytes = Zeroizing::new(Vec::new());
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| unavailable())?;
        if bytes.len() > 4096 {
            return Err(unavailable());
        }
        let state = child.wait().await.map_err(|_| unavailable())?;
        if !state.success() {
            return Err(unavailable());
        }
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| unavailable())?
            .trim();
        if value.is_empty()
            || value.chars().any(char::is_whitespace)
            || reqwest::header::HeaderValue::from_str(value).is_err()
        {
            return Err(unavailable());
        }
        Ok(Secret::new(value.into()))
    };
    tokio::select! {
        _ = stop.cancelled() => Err(unavailable()),
        result = tokio::time::timeout(Duration::from_secs(15), operation) =>
            result.map_err(|_| unavailable())?,
    }
}

fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "GitHub CLI login could not be imported",
    )
}
