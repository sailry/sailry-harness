//! Local credential sources use fixed argv and a minimal environment, never a shell.
use super::*;
use std::{path::Path, process::Stdio};
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

pub(super) async fn identity(
    auth: &Auth,
    environment: &crate::process::Environment,
    stop: &CancellationToken,
) -> Result<Identity, Fault> {
    let Auth::LocalCli {
        command,
        env,
        status,
        timeout_ms,
        max_output,
        ..
    } = auth
    else {
        return Err(unavailable());
    };
    let path = which::which_in(
        command,
        environment.path(),
        environment.home().unwrap_or(Path::new(".")),
    )
    .map_err(|_| unavailable())?;
    let account = account(
        &run(
            &path,
            &status.argv,
            env,
            environment,
            *timeout_ms,
            *max_output,
            stop,
        )
        .await?,
        status,
    )?;
    Ok(Identity {
        name: account.clone(),
        id: account,
    })
}

pub(super) async fn credentials(
    auth: &Auth,
    environment: &crate::process::Environment,
    expected: Option<&str>,
    stop: &CancellationToken,
) -> Result<(Identity, Secret), Fault> {
    let Auth::LocalCli {
        command,
        env,
        status,
        token,
        timeout_ms,
        max_output,
    } = auth
    else {
        return Err(unavailable());
    };
    let path = which::which_in(
        command,
        environment.path(),
        environment.home().unwrap_or(Path::new(".")),
    )
    .map_err(|_| unavailable())?;
    let before = account(
        &run(
            &path,
            &status.argv,
            env,
            environment,
            *timeout_ms,
            *max_output,
            stop,
        )
        .await?,
        status,
    )?;
    if expected.is_some_and(|expected| expected != before) {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "local account changed; sign in again",
        ));
    }
    let argv = token
        .argv
        .iter()
        .map(|value| value.replace("{account}", &before))
        .collect::<Vec<_>>();
    let bytes = run(
        &path,
        &argv,
        env,
        environment,
        *timeout_ms,
        *max_output,
        stop,
    )
    .await?;
    let value = match &token.parse {
        Parse::Line => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
            .map(str::to_owned),
        Parse::Json(pointer) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            }),
    }
    .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
    .ok_or_else(unavailable)?;
    let after = account(
        &run(
            &path,
            &status.argv,
            env,
            environment,
            *timeout_ms,
            *max_output,
            stop,
        )
        .await?,
        status,
    )?;
    if before != after {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "local account changed; sign in again",
        ));
    }
    Ok((
        Identity {
            name: before.clone(),
            id: before,
        },
        Secret::new(value),
    ))
}

fn account(bytes: &[u8], status: &CliStatus) -> Result<String, Fault> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| unavailable())?;
    if status.condition.as_ref().is_some_and(|condition| {
        value
            .pointer(&condition.pointer)
            .is_none_or(|actual| !condition.values.contains(actual))
    }) {
        return Err(unavailable());
    }
    value
        .pointer(&status.account)
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"._@-".contains(&value))
        })
        .map(str::to_owned)
        .ok_or_else(unavailable)
}

async fn run(
    path: &Path,
    argv: &[String],
    env: &BTreeMap<String, String>,
    environment: &crate::process::Environment,
    timeout: u64,
    limit: usize,
    stop: &CancellationToken,
) -> Result<Zeroizing<Vec<u8>>, Fault> {
    let mut command = tokio::process::Command::new(path);
    command
        .env_clear()
        .args(argv)
        .envs(env)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    if let Some(path) = environment.path() {
        command.env("PATH", path);
    }
    if let Some(home) = environment.home() {
        command.env("HOME", home);
    }
    for name in [
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "SystemRoot",
        "SystemDrive",
        "PATHEXT",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let operation = async {
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(unavailable)?
            .take(limit as u64 + 1);
        let mut bytes = Zeroizing::new(Vec::new());
        output
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| unavailable())?;
        if bytes.len() > limit || !child.wait().await.map_err(|_| unavailable())?.success() {
            return Err(unavailable());
        }
        Ok(bytes)
    };
    tokio::select! {biased;_=stop.cancelled()=>Err(cancelled()),result=tokio::time::timeout(Duration::from_millis(timeout),operation)=>result.map_err(|_|unavailable())?}
}

#[cfg(test)]
mod tests;
