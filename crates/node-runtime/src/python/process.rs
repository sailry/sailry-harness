use super::{cancelled, check};
use crate::process::Environment;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

pub(super) async fn run(
    python: &Path,
    args: &[String],
    directory: &Path,
    environment: &Environment,
    variables: &[(&str, OsString)],
    cancel: &CancellationToken,
) -> Result<(), Fault> {
    check(cancel)?;
    let mut command = tokio::process::Command::new(python);
    environment.apply(&mut command);
    command
        .arg("-B")
        .args(args)
        .current_dir(directory)
        .envs(variables.iter().map(|(key, value)| (*key, value)))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut command = crate::process::command(command);
    let mut child = command.spawn().map_err(|error| {
        Fault::new(
            ErrorCode::Unavailable,
            format!("Python preparation could not start: {error}"),
        )
    })?;
    let mut stderr = child.stderr().take().ok_or_else(super::unavailable)?;
    let mut errors = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut buffer = [0u8; 4096];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..count]);
            if tail.len() > 16 * 1024 {
                tail.drain(..tail.len() - 16 * 1024);
            }
        }
        tail
    });
    let wait = async {
        // SAFETY: this owner retains the exact ProcessGroup/JobObject wrapper;
        // waiting for its native root does not transfer group cleanup ownership.
        unsafe { child.try_inner_child_mut() }
            .expect("Python child is native")
            .wait()
            .await
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(cancelled()),
        result = tokio::time::timeout(Duration::from_secs(20 * 60), wait) => result
            .map_err(|_| Fault::new(ErrorCode::Unavailable, "Python preparation timed out"))
            .and_then(|result| result.map_err(|error| Fault::new(ErrorCode::Unavailable, format!("Python preparation wait failed: {error}")))),
    };
    let _ = child.start_kill();
    // SAFETY: the same owner observes its retained native root after group cleanup.
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        unsafe { child.try_inner_child_mut() }
            .expect("Python child is native")
            .wait(),
    )
    .await;
    let tail = match tokio::time::timeout(Duration::from_secs(1), &mut errors).await {
        Ok(Ok(tail)) => tail,
        _ => {
            errors.abort();
            let _ = errors.await;
            Vec::new()
        }
    };
    let status = result?;
    if !status.success() {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            format!(
                "Python preparation exited with {status}: {}",
                String::from_utf8_lossy(&tail)
            ),
        ));
    }
    check(cancel)
}
