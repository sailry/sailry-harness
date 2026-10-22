//! Permission diagnostics use the same captured helper configuration as MCP.
use crate::{plugins::resources::mcp::Launch, process::Environment};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, NodeId,
    computer::{Permission, Permissions},
};

pub(crate) async fn read(
    node: NodeId,
    caller: NodeId,
    launch: Option<Launch>,
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<Permissions, Fault> {
    #[cfg(target_os = "macos")]
    let state = {
        let launch = launch.ok_or_else(unconfigured)?;
        let bytes = run(&launch, environment, true, stop, DEADLINE).await?;
        Some(parse(&bytes)?)
    };
    #[cfg(not(target_os = "macos"))]
    let state: Option<Grants> = {
        let _ = (launch, environment, stop);
        None
    };
    Ok(Permissions {
        node,
        platform: std::env::consts::OS.into(),
        local: node == caller,
        screen_capture: state.as_ref().map(|state| state.screen_capture),
        accessibility: state.as_ref().map(|state| state.accessibility),
    })
}

pub(crate) async fn request(
    node: NodeId,
    caller: NodeId,
    permission: Permission,
    launch: Option<Launch>,
    environment: &Environment,
    stop: &CancellationToken,
) -> Result<Permissions, Fault> {
    if node != caller {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "system permissions must be granted on the execution device",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let launch = launch.ok_or_else(unconfigured)?;
        let current = parse(&run(&launch, environment, true, stop, DEADLINE).await?)?;
        let granted = match permission {
            Permission::ScreenCapture => current.screen_capture,
            Permission::Accessibility => current.accessibility,
        };
        if granted {
            return Ok(Permissions {
                node,
                platform: std::env::consts::OS.into(),
                local: true,
                screen_capture: Some(current.screen_capture),
                accessibility: Some(current.accessibility),
            });
        }
        // This upstream command opens overall onboarding, not one TCC pane.
        run(&launch, environment, false, stop, DEADLINE).await?;
        read(node, caller, Some(launch), environment, stop).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (permission, launch, environment, stop);
        Err(Fault::new(
            ErrorCode::Unavailable,
            "computer permission setup is unavailable on this platform",
        ))
    }
}

#[derive(serde::Deserialize)]
struct Grants {
    #[serde(rename = "screenCaptureGranted")]
    screen_capture: bool,
    #[serde(rename = "accessibilityTrusted")]
    accessibility: bool,
}

#[cfg(any(test, target_os = "macos"))]
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(any(test, target_os = "macos"))]
fn parse(bytes: &[u8]) -> Result<Grants, Fault> {
    serde_json::from_slice(bytes).map_err(|_| {
        Fault::new(
            ErrorCode::Unavailable,
            "computer permission diagnostics are malformed",
        )
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn unconfigured() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "computer use requires a captured helper configuration",
    )
}

#[cfg(any(test, target_os = "macos"))]
async fn run(
    launch: &Launch,
    environment: &Environment,
    json: bool,
    stop: &CancellationToken,
    deadline: std::time::Duration,
) -> Result<Vec<u8>, Fault> {
    use std::{process::Stdio, time::Duration};
    use tokio::io::AsyncReadExt;

    const MAX_BYTES: usize = 64 * 1024;
    let failure = || {
        Fault::new(
            ErrorCode::Unavailable,
            "computer permission diagnostics failed",
        )
    };
    let Launch::Stdio {
        command, env, cwd, ..
    } = launch
    else {
        return Err(Fault::invalid(
            "computer permission diagnostics require a helper",
        ));
    };
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "computer permission check was cancelled",
        ));
    }
    let mut command = tokio::process::Command::new(command);
    environment.apply(&mut command);
    command
        .arg("doctor")
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if json {
        command.arg("--json");
    }
    let mut child = command.spawn().map_err(|_| failure())?;
    let operation = async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(failure)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failure())?;
        if bytes.len() > MAX_BYTES {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "computer permission diagnostics exceed the output limit",
            ));
        }
        if !child.wait().await.map_err(|_| failure())?.success() {
            return Err(failure());
        }
        Ok(bytes)
    };
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "computer permission check was cancelled")),
        result = tokio::time::timeout(deadline, operation) => match result {
            Ok(result) => result,
            Err(_) => Err(Fault::new(ErrorCode::Unavailable, "computer permission diagnostics timed out")),
        },
    };
    if result.is_err() {
        let _ = child.start_kill();
        if !matches!(
            tokio::time::timeout(Duration::from_secs(2), child.wait()).await,
            Ok(Ok(_))
        ) {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "computer permission diagnostics could not be stopped",
            ));
        }
    }
    result
}

#[cfg(test)]
mod tests;
