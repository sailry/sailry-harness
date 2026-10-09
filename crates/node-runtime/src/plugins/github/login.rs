//! CLI bindings keep account identity in Sailry and credentials in GitHub CLI storage.
use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    pub(crate) login: String,
    pub(crate) source: String,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) identity: Identity,
    pub(crate) fingerprint: blake3::Hash,
}

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
    for name in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        command.env_remove(name);
    }
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    command
}

async fn output(
    mut command: tokio::process::Command,
    limit: u64,
    stop: &CancellationToken,
) -> Result<Zeroizing<Vec<u8>>, Fault> {
    command.stdout(Stdio::piped());
    let operation = async {
        let mut child = command.spawn().map_err(|_| unavailable())?;
        let mut stdout = child.stdout.take().ok_or_else(unavailable)?.take(limit + 1);
        let mut bytes = Zeroizing::new(Vec::new());
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| unavailable())?;
        if bytes.len() as u64 > limit {
            return Err(unavailable());
        }
        if !child.wait().await.map_err(|_| unavailable())?.success() {
            return Err(unavailable());
        }
        Ok(bytes)
    };
    tokio::select! {
        _ = stop.cancelled() => Err(unavailable()),
        result = tokio::time::timeout(Duration::from_secs(15), operation) => result.map_err(|_| unavailable())?,
    }
}

pub(crate) async fn status(
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<(bool, Option<Identity>), Fault> {
    let Some(path) = executable(environment) else {
        return Ok((false, None));
    };
    let mut command = command(environment, &path);
    // gh 2.96.0 omits tokens in JSON unless --show-token is explicitly requested.
    command.args([
        "auth",
        "status",
        "--active",
        "--hostname",
        "github.com",
        "--json",
        "hosts",
    ]);
    let bytes = output(command, 16 * 1024, stop).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    let mut entries = value
        .pointer("/hosts/github.com")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry["active"] == true && entry["host"] == "github.com" && entry["state"] == "success"
        });
    let identity = entries
        .next()
        .map(|entry| {
            let login = entry["login"].as_str().ok_or_else(unavailable)?;
            let source = entry["tokenSource"].as_str().ok_or_else(unavailable)?;
            if login.is_empty()
                || login.len() > 100
                || !login
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || source.is_empty()
                || source.len() > 4096
                || source.chars().any(char::is_control)
                || source.ends_with("_TOKEN")
            {
                return Err(unavailable());
            }
            Ok(Identity {
                login: login.into(),
                source: source.into(),
            })
        })
        .transpose()?;
    if entries.next().is_some() {
        return Err(unavailable());
    }
    Ok((true, identity))
}

pub(crate) async fn import(
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<Identity, Fault> {
    let identity = status(environment, stop)
        .await?
        .1
        .ok_or_else(not_configured)?;
    credential(environment, &identity, stop).await?;
    Ok(identity)
}

pub(crate) async fn credential(
    environment: &Environment,
    identity: &Identity,
    stop: &CancellationToken,
) -> Result<Secret, Fault> {
    let path = executable(environment).ok_or_else(not_configured)?;
    let mut command = command(environment, &path);
    command.args([
        "auth",
        "token",
        "--hostname",
        "github.com",
        "--user",
        &identity.login,
    ]);
    let result = output(command, 4096, stop).await;
    if status(environment, stop).await?.1.as_ref() != Some(identity) {
        return Err(not_configured());
    }
    let bytes = result?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| unavailable())?
        .trim();
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || reqwest::header::HeaderValue::from_str(value).is_err()
    {
        return Err(unavailable());
    }
    let token = Secret::new(value.into());
    Ok(token)
}

impl Binding {
    pub(crate) async fn validate(
        &self,
        environment: &Environment,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let token = credential(environment, &self.identity, stop).await?;
        if blake3::hash(token.expose().as_bytes()) != self.fingerprint {
            return Err(not_configured());
        }
        Ok(())
    }
}

fn not_configured() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "GitHub CLI account is signed out or changed; link the active account again",
    )
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "GitHub CLI login is unavailable")
}

#[cfg(test)]
mod tests;
