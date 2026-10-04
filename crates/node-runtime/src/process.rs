//! Approved commands use a managed process group/Job, not the interactive PTY.
//! Behavioral reference: sailry-code 67ae9fa0, terminal-host/src/headless.
pub(crate) mod background;
#[path = "process/output.rs"]
pub(crate) mod output;
#[cfg(test)]
#[path = "process/tests.rs"]
mod tests;

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, process::*};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::{Duration, Instant},
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const ENVIRONMENT: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "PATH",
    "PATHEXT",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SystemRoot",
    "SystemDrive",
    "COMSPEC",
    "TEMP",
    "TMP",
    "TMPDIR",
];

pub(crate) struct Launch {
    pub root: PathBuf,
    pub command: String,
    pub cwd: String,
    pub timeout_ms: u64,
    pub inputs: Option<crate::files::attachments::Inputs>,
}

impl Launch {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.command.trim().is_empty()
            || self.command.len() > 64 * 1024
            || self.command.contains('\0')
            || !(1..=900_000).contains(&self.timeout_ms)
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid command or timeout",
            ));
        }
        if self.cwd.len() > 4096 || self.cwd.chars().any(char::is_control) {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid command working directory",
            ));
        }
        Ok(())
    }

    fn directory(&self) -> Result<PathBuf, Fault> {
        self.validate()?;
        // Retain the registered worktree check, but do not apply file-operation
        // confinement to an approved shell's explicitly selected directory.
        let _root = crate::files::path::root(&self.root)?;
        let directory = self.root.join(&self.cwd).canonicalize().map_err(|error| {
            Fault::new(
                ErrorCode::InvalidRequest,
                format!("command working directory is unavailable: {error}"),
            )
        })?;
        if !directory.is_dir() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "command working directory is not a directory",
            ));
        }
        Ok(directory)
    }
}

pub(crate) struct Environment(BTreeMap<OsString, OsString>);

impl Environment {
    pub(crate) fn apply(&self, command: &mut tokio::process::Command) {
        command.env_clear().envs(&self.0);
    }

    pub fn capture() -> Self {
        Self(
            ENVIRONMENT
                .iter()
                .filter_map(|name| {
                    std::env::var_os(name).map(|value| (OsString::from(name), value))
                })
                .collect(),
        )
    }

    fn command(
        &self,
        launch: &Launch,
        directory: &Path,
        inputs: Option<&Path>,
    ) -> tokio::process::Command {
        #[cfg(unix)]
        let mut command = {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args(["-c", &launch.command]);
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = tokio::process::Command::new(
                self.0
                    .get(std::ffi::OsStr::new("COMSPEC"))
                    .cloned()
                    .unwrap_or_else(|| "cmd.exe".into()),
            );
            command.args(["/D", "/S", "/C", &launch.command]);
            command
        };
        self.apply(&mut command);
        if let Some(inputs) = inputs {
            command.env("SAILRY_ATTACHMENTS", inputs);
        }
        command
            .current_dir(directory)
            .env("PWD", directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

struct Child {
    inner: Box<dyn ChildWrapper>,
    armed: bool,
}

impl Child {
    async fn wait_root(&mut self) -> std::io::Result<ExitStatus> {
        // SAFETY: only the reviewed ProcessGroup/JobObject wrapper owns this native
        // child. We never call wrapper wait or mutate its group/Job; wrapper cleanup
        // remains active while Tokio caches and reaps the exact root process.
        unsafe { self.inner.try_inner_child_mut() }
            .expect("command owns a native process")
            .wait()
            .await
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        // KillOnDrop alone only kills the root on Unix; retain group cleanup on unwinding.
        if self.armed {
            let _ = self.inner.start_kill();
        }
    }
}

pub(crate) async fn execute(
    launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Completion, Fault> {
    execute_observed(launch, environment, stop, closed, None).await
}

pub(crate) async fn execute_observed(
    mut launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    progress: Option<&tokio::sync::watch::Sender<(Capture, Capture)>>,
) -> Result<Completion, Fault> {
    let started = Instant::now();
    if stop.is_cancelled() || closed.is_cancelled() {
        return Ok(cancelled());
    }
    let directory = launch.directory()?;
    let inputs = match launch.inputs.take() {
        Some(inputs) => Some(inputs.stage(stop.clone()).await?),
        None => None,
    };
    let mut command = CommandWrap::from(environment.command(
        &launch,
        &directory,
        inputs.as_ref().map(|directory| directory.path()),
    ));
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    if stop.is_cancelled() || closed.is_cancelled() {
        return Ok(cancelled());
    }
    let mut child = Child {
        inner: command.spawn().map_err(|error| {
            Fault::new(
                ErrorCode::Unavailable,
                format!("command launch failed: {error}"),
            )
        })?,
        armed: true,
    };
    let stdout = child
        .inner
        .stdout()
        .take()
        .expect("command stdout was piped");
    let stderr = child
        .inner
        .stderr()
        .take()
        .expect("command stderr was piped");
    let mut capture = output::Drain::with_tail(stdout, stderr, progress.is_some());
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut outcome = {
        let wait = child.wait_root();
        tokio::pin!(wait);
        let timeout = tokio::time::sleep(Duration::from_millis(launch.timeout_ms));
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                biased;
                _ = closed.cancelled() => break Outcome::Cancelled,
                _ = stop.cancelled() => break Outcome::Cancelled,
                _ = &mut timeout, if progress.is_none() => break Outcome::TimedOut,
                result = &mut wait => break match result {
                    Ok(status) => exited(status),
                    Err(error) => Outcome::Unknown(format!("command wait failed: {error}")),
                },
                _ = tick.tick(), if progress.is_some() => {
                    progress.unwrap().send_replace(capture.snapshot());
                }
            }
        }
    };
    // Terminate remaining members of this command's group/Job after root exit, too.
    let cleanup = child.inner.start_kill();
    child.armed = false;
    if let Err(error) = cleanup {
        #[cfg(unix)]
        let absent = error.raw_os_error() == Some(libc::ESRCH);
        #[cfg(not(unix))]
        let absent = false;
        if !absent {
            outcome = Outcome::Unknown(format!("command cleanup failed: {error}"));
        }
    }
    // Wait on the retained root handle. Wrapper wait may block on a background Job;
    // this command has already terminated that group instead of waiting for daemon exit.
    match tokio::time::timeout(CLEANUP_TIMEOUT, child.wait_root()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => outcome = Outcome::Unknown(format!("command reap failed: {error}")),
        Err(_) => outcome = Outcome::Unknown("command cleanup timed out".into()),
    }
    let (stdout, stderr, error) = capture.finish(CLEANUP_TIMEOUT).await;
    if let Some(error) = error {
        outcome = Outcome::Unknown(error);
    }
    Ok(Completion {
        outcome,
        stdout,
        stderr,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

pub(crate) fn cancelled() -> Completion {
    Completion {
        outcome: Outcome::Cancelled,
        stdout: Capture::default(),
        stderr: Capture::default(),
        elapsed_ms: 0,
    }
}

fn exited(status: ExitStatus) -> Outcome {
    if let Some(code) = status.code() {
        return Outcome::Exited(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Outcome::Signal(signal);
        }
    }
    Outcome::Unknown("command exit status is unavailable".into())
}
