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
        parse(&run(&launch, environment, false, stop, DEADLINE).await?)?
    };
    #[cfg(not(target_os = "macos"))]
    let state = {
        let _ = (launch, environment, stop);
        Grants::default()
    };
    Ok(Permissions {
        node,
        platform: std::env::consts::OS.into(),
        local: node == caller,
        screen_capture: state.screen_capture,
        accessibility: state.accessibility,
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
        let current = parse(&run(&launch, environment, false, stop, DEADLINE).await?)?;
        let granted = match permission {
            Permission::ScreenCapture => current.screen_capture,
            Permission::Accessibility => current.accessibility,
        };
        if granted != Some(false) {
            return Ok(Permissions {
                node,
                platform: std::env::consts::OS.into(),
                local: true,
                screen_capture: current.screen_capture,
                accessibility: current.accessibility,
            });
        }
        // The helper prompts for every missing grant; a fresh process reads the
        // result because Screen Recording state is fixed at process launch.
        run(&launch, environment, true, stop, DEADLINE).await?;
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

/// Grants the helper reports; `None` means not required or undetermined.
#[derive(Debug, Default, PartialEq, Eq)]
struct Grants {
    screen_capture: Option<bool>,
    accessibility: Option<bool>,
}

#[cfg(any(test, target_os = "macos"))]
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

/// Read the `agent-desktop permissions` JSON envelope.
#[cfg(any(test, target_os = "macos"))]
fn parse(bytes: &[u8]) -> Result<Grants, Fault> {
    #[derive(serde::Deserialize)]
    struct Envelope {
        ok: bool,
        command: String,
        data: Option<Report>,
    }
    #[derive(serde::Deserialize)]
    struct Report {
        accessibility: State,
        screen_recording: State,
    }
    #[derive(serde::Deserialize)]
    struct State {
        state: String,
    }
    let malformed = || {
        Fault::new(
            ErrorCode::Unavailable,
            "computer permission diagnostics are malformed",
        )
    };
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| malformed())?;
    if !envelope.ok || envelope.command != "permissions" {
        return Err(malformed());
    }
    let report = envelope.data.ok_or_else(malformed)?;
    let grant = |state: State| match state.state.as_str() {
        "granted" => Ok(Some(true)),
        "denied" => Ok(Some(false)),
        "not_required" | "unknown" => Ok(None),
        _ => Err(malformed()),
    };
    Ok(Grants {
        screen_capture: grant(report.screen_recording)?,
        accessibility: grant(report.accessibility)?,
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn unconfigured() -> Fault {
    Fault::new(
        ErrorCode::NotConfigured,
        "computer use requires a captured helper configuration",
    )
}

#[cfg(any(target_os = "macos", all(test, unix)))]
async fn run(
    launch: &Launch,
    environment: &Environment,
    request: bool,
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
        command,
        env,
        cwd,
        computer,
        ..
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
        .arg("permissions")
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if request {
        command.arg("--request");
    }
    #[cfg(target_os = "macos")]
    if *computer {
        crate::computer::helper::disclaim(&mut command);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = computer;
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
