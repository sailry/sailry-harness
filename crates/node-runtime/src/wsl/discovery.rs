use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use std::{process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

const LIMIT: usize = 64 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);
const CLEANUP: Duration = Duration::from_secs(2);

#[cfg(windows)]
pub(super) async fn list(
    executable: &std::path::Path,
    environment: &crate::process::Environment,
    stop: &CancellationToken,
) -> Result<Vec<String>, Fault> {
    let registered = tokio::task::spawn_blocking(|| {
        registered(
            windows_registry::CURRENT_USER.open(r"Software\Microsoft\Windows\CurrentVersion\Lxss"),
        )
    })
    .await
    .map_err(|_| failure("registration discovery worker failed"))??;
    if !registered {
        return Ok(Vec::new());
    }
    let mut command = tokio::process::Command::new(executable);
    environment.apply(&mut command);
    command.args(["--list", "--quiet"]);
    command.current_dir(
        executable
            .parent()
            .ok_or_else(|| failure("executable path is invalid"))?,
    );
    let bytes = query(command, stop, DEADLINE).await?;
    distributions(&bytes)
}

#[cfg(windows)]
pub(super) fn versions(
    names: Vec<String>,
) -> Result<Vec<sailry_protocol::wsl::Distribution>, Fault> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let read_error = |error: windows::core::Error| {
        failure(format!(
            "distribution registration read failed (HRESULT 0x{:08X})",
            error.code().0 as u32
        ))
    };
    let root = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Lxss")
        .map_err(read_error)?;
    metadata(&root, names)
}

#[cfg(windows)]
fn metadata(
    root: &windows_registry::Key,
    names: Vec<String>,
) -> Result<Vec<sailry_protocol::wsl::Distribution>, Fault> {
    let read_error = |error: windows::core::Error| {
        failure(format!(
            "distribution registration read failed (HRESULT 0x{:08X})",
            error.code().0 as u32
        ))
    };
    let mut versions = std::collections::BTreeMap::new();
    for key in root.keys().map_err(read_error)? {
        if key.len() != 38 || uuid::Uuid::parse_str(&key).is_err() {
            continue;
        }
        let key = root.open(key).map_err(read_error)?;
        let name = key.get_string("DistributionName").map_err(read_error)?;
        if names.contains(&name) {
            versions.insert(name, key.get_u32("Version").map_err(read_error)?);
        }
    }
    names
        .into_iter()
        .map(|name| {
            let version = versions.remove(&name).ok_or_else(|| {
                failure("distribution registration changed; refresh the distribution list")
            })?;
            Ok(sailry_protocol::wsl::Distribution { name, version })
        })
        .collect()
}

#[cfg(windows)]
fn registered(root: windows_registry::Result<windows_registry::Key>) -> Result<bool, Fault> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    let key = match root {
        Ok(key) => key,
        Err(error)
            if [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND]
                .into_iter()
                .any(|code| error.code().0 == windows::core::HRESULT::from_win32(code).0) =>
        {
            return Ok(false);
        }
        Err(error) => {
            return Err(failure(format!(
                "registration read failed (HRESULT 0x{:08X})",
                error.code().0 as u32
            )));
        }
    };
    let mut keys = key.keys().map_err(|error| {
        failure(format!(
            "registration enumeration failed (HRESULT 0x{:08X})",
            error.code().0 as u32
        ))
    })?;
    Ok(keys.any(|name| name.len() == 38 && uuid::Uuid::parse_str(&name).is_ok()))
}

async fn query(
    mut command: tokio::process::Command,
    stop: &CancellationToken,
    deadline: Duration,
) -> Result<Vec<u8>, Fault> {
    if stop.is_cancelled() {
        return Err(Fault::new(
            ErrorCode::Cancelled,
            "WSL discovery was cancelled",
        ));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    let mut command = crate::process::command(command);
    let mut child = crate::process::Child {
        inner: command
            .spawn()
            .map_err(|error| failure(format!("launch failed: {error}")))?,
        armed: true,
    };
    let mut pipe = child
        .inner
        .stdout()
        .take()
        .ok_or_else(|| failure("output pipe is unavailable"))?
        .take((LIMIT + 1) as u64);
    let operation = async {
        let output = async {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes)
                .await
                .map_err(|error| failure(format!("output read failed: {error}")))?;
            if bytes.len() > LIMIT {
                return Err(failure("output exceeds the supported size"));
            }
            Ok(bytes)
        };
        let wait = async {
            child
                .wait_root()
                .await
                .map_err(|error| failure(format!("wait failed: {error}")))
        };
        let (status, bytes) = tokio::try_join!(wait, output)?;
        if !status.success() {
            return Err(match status.code() {
                Some(code) => failure(format!(
                    "failed with exit code {code} (0x{:08X})",
                    code as u32
                )),
                None => failure("terminated without an exit code"),
            });
        }
        Ok(bytes)
    };
    let result = tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "WSL discovery was cancelled")),
        result = tokio::time::timeout(deadline, operation) => match result {
            Ok(result) => result,
            Err(_) => Err(failure("timed out")),
        },
    };
    let _ = child.inner.start_kill();
    if !matches!(
        tokio::time::timeout(CLEANUP, child.wait_root()).await,
        Ok(Ok(_))
    ) {
        return Err(failure("process could not be stopped"));
    }
    result
}

fn distributions(bytes: &[u8]) -> Result<Vec<String>, Fault> {
    let invalid = || failure("distribution list is malformed");
    let text = if bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || bytes.contains(&0)
    {
        let big_endian = bytes.starts_with(&[0xfe, 0xff]);
        let bytes = if bytes.starts_with(&[0xff, 0xfe]) || big_endian {
            &bytes[2..]
        } else {
            bytes
        };
        if !bytes.len().is_multiple_of(2) {
            return Err(invalid());
        }
        let (units, _) = bytes.as_chunks::<2>();
        let units = units
            .iter()
            .map(|&unit| {
                if big_endian {
                    u16::from_be_bytes(unit)
                } else {
                    u16::from_le_bytes(unit)
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| invalid())?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| invalid())?
            .trim_start_matches('\u{feff}')
            .to_owned()
    };
    text.trim_end_matches('\0')
        .lines()
        .filter(|name| !name.is_empty())
        .map(|name| {
            if name.chars().any(char::is_control) {
                return Err(invalid());
            }
            Ok(name.to_owned())
        })
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map(|names| names.into_iter().collect())
}

fn failure(message: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, format!("WSL discovery {message}"))
}

#[cfg(test)]
mod tests;
