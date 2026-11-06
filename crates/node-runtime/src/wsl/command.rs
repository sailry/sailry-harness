use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const LIMIT: usize = 128 * 1024;
const CLEANUP: Duration = Duration::from_secs(2);

pub(super) async fn run(
    mut command: tokio::process::Command,
    script: &str,
    stop: &CancellationToken,
    deadline: Duration,
) -> Result<String, Fault> {
    if stop.is_cancelled() {
        return Err(cancelled());
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let mut command = crate::process::command(command);
    let mut child = crate::process::Child {
        inner: command
            .spawn()
            .map_err(|error| unavailable(format!("WSL launch failed: {error}")))?,
        armed: true,
    };
    let mut stdin = child.inner.stdin().take().expect("WSL input was piped");
    let stdout = child.inner.stdout().take().expect("WSL output was piped");
    let stderr = child
        .inner
        .stderr()
        .take()
        .expect("WSL diagnostics were piped");
    let operation = async {
        let input = async move {
            stdin
                .write_all(script.as_bytes())
                .await
                .map_err(|_| unknown("WSL command input was interrupted"))?;
            drop(stdin);
            Ok::<(), Fault>(())
        };
        let wait = async {
            child
                .wait_root()
                .await
                .map_err(|_| unknown("WSL command status is unavailable"))
        };
        let (status, (), stdout, stderr) =
            tokio::try_join!(wait, input, read(stdout), read(stderr))?;
        if !status.success() {
            return Err(if status.code().is_none_or(|code| code == 73 || code < 0) {
                unknown("Host setup is unconfirmed; inspect the distribution before retrying")
            } else {
                unavailable(format!(
                    "WSL command failed ({}): {}",
                    status
                        .code()
                        .map_or_else(|| "no exit code".into(), |code| code.to_string()),
                    String::from_utf8_lossy(&stderr).trim()
                ))
            });
        }
        String::from_utf8(stdout).map_err(|_| unknown("WSL command output is not UTF-8"))
    };
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(cancelled()),
        result = tokio::time::timeout(deadline, operation) => result.unwrap_or_else(|_| Err(unknown("WSL command timed out; inspect the distribution before retrying"))),
    };
    let _ = child.inner.start_kill();
    if !matches!(
        tokio::time::timeout(CLEANUP, child.wait_root()).await,
        Ok(Ok(_))
    ) {
        return Err(unknown(
            "WSL command could not be stopped; inspect the distribution before retrying",
        ));
    }
    result
}

async fn read(stream: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, Fault> {
    let mut bytes = Vec::new();
    stream
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| unknown("WSL output read failed"))?;
    if bytes.len() > LIMIT {
        return Err(unknown("WSL command output exceeds the supported size"));
    }
    Ok(bytes)
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "WSL Host installation was cancelled")
}

fn unavailable(message: impl Into<String>) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

fn unknown(message: &str) -> Fault {
    Fault::new(ErrorCode::OutcomeUnknown, message)
}

#[cfg(all(test, unix))]
mod tests;
