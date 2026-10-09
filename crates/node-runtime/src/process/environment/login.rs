// Adapted from sailry-code 67ae9fa0, terminal-host/src/shell.rs.
// See third_party_licenses/sailry-code-terminal.md.
use super::LIMIT;
use crate::process::Child;
use process_wrap::tokio::{CommandWrap, KillOnDrop, ProcessGroup};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

pub(super) async fn capture(
    shell: &OsStr,
    environment: &BTreeMap<OsString, OsString>,
    deadline: tokio::time::Instant,
) -> Option<BTreeMap<OsString, OsString>> {
    if tokio::time::Instant::now() >= deadline {
        return None;
    }
    let marker = format!("SAILRY_ENV_{}", uuid::Uuid::new_v4());
    let script = format!("printf '\\0{marker}\\0'; /usr/bin/env -0 && printf '\\0{marker}-end\\0'");
    let mut command = tokio::process::Command::new(shell);
    match Path::new(shell).file_name().and_then(OsStr::to_str) {
        Some("fish") => {
            command.args(["-l", "-i", "-c", &script]);
        }
        Some("bash" | "zsh" | "sh") => {
            command.args(["-ilc", &script]);
        }
        _ => {
            command.args(["-l", "-c", &script]);
        }
    }
    command
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = environment.get(OsStr::new("HOME")) {
        command.current_dir(home);
    }
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop).wrap(ProcessGroup::leader());
    let mut child = Child {
        inner: command.spawn().ok()?,
        armed: true,
    };
    let pipe = child.inner.stdout().take()?;
    let reserve = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .div_f32(5.0)
        .min(Duration::from_millis(100));
    let probe_deadline = deadline - reserve;
    let captured = tokio::time::timeout_at(probe_deadline, async {
        let mut bytes = Vec::new();
        let mut pipe = pipe.take((LIMIT + 1) as u64);
        let root = async {
            let status = child.wait_root().await?;
            let _ = child.inner.start_kill();
            Ok::<_, std::io::Error>(status)
        };
        let (status, read) = tokio::try_join!(root, pipe.read_to_end(&mut bytes))?;
        Ok::<_, std::io::Error>((status.success() && read <= LIMIT).then_some(bytes))
    })
    .await
    .ok()
    .and_then(Result::ok)
    .flatten();
    // Kill descendants even after the shell exits; retain the exact child for bounded reaping.
    let _ = child.inner.start_kill();
    let _ = tokio::time::timeout_at(deadline, child.wait_root()).await;
    captured.and_then(|bytes| parse(&bytes, marker.as_bytes()))
}

pub(super) fn parse(bytes: &[u8], marker: &[u8]) -> Option<BTreeMap<OsString, OsString>> {
    let mut start = vec![0];
    start.extend_from_slice(marker);
    start.push(0);
    let offset = bytes
        .windows(start.len())
        .position(|bytes| bytes == start)?
        + start.len();
    let mut end = vec![0];
    end.extend_from_slice(marker);
    end.extend_from_slice(b"-end\0");
    let content = &bytes[offset..];
    let end = content.windows(end.len()).position(|bytes| bytes == end)?;
    let mut values = BTreeMap::new();
    for record in content[..end]
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let separator = record.iter().position(|byte| *byte == b'=')?;
        if separator == 0 {
            return None;
        }
        values.insert(
            OsString::from_vec(record[..separator].to_vec()),
            OsString::from_vec(record[separator + 1..].to_vec()),
        );
    }
    Some(values)
}
