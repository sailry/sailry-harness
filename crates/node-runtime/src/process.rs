//! Approved commands retain group/Job cleanup with optional native PTY I/O.
//! Behavioral reference: sailry-code 67ae9fa0, terminal-host/src/headless.
pub(crate) mod background;
pub(crate) mod environment;
mod interactive;
#[cfg(target_os = "macos")]
#[path = "process/macos.rs"]
mod macos;
pub(crate) mod pty;
pub(crate) use environment::Environment;
#[cfg(windows)]
pub(crate) use interactive::windows::Job as PtyJob;
pub(crate) mod output;
pub(crate) mod sandbox;
#[cfg(test)]
#[path = "process/tests.rs"]
mod tests;

use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, process::*};
use std::{
    path::PathBuf,
    process::ExitStatus,
    sync::Arc,
    time::{Duration, Instant},
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Every owned native process uses the same group/Job and root cleanup policy.
pub(crate) fn command(command: tokio::process::Command) -> CommandWrap {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    command
}

pub(crate) struct Launch {
    pub root: PathBuf,
    pub profile: Option<PathBuf>,
    pub command: String,
    pub access: Access,
    pub pty: bool,
    pub cwd: String,
    pub timeout_ms: u64,
    pub inputs: Option<crate::files::attachments::Inputs>,
    pub scratch: Option<Arc<tempfile::TempDir>>,
    pub python: Option<crate::python::Execution>,
}

impl Launch {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.command.trim().is_empty()
            || self.command.contains('\0')
            || self.timeout_ms == 0
            || Instant::now()
                .checked_add(Duration::from_millis(self.timeout_ms))
                .is_none()
        {
            return Err(Fault::invalid("invalid command or timeout"));
        }
        if self.cwd.chars().any(char::is_control) {
            return Err(Fault::invalid("invalid command working directory"));
        }
        Ok(())
    }

    fn directory(&self) -> Result<PathBuf, Fault> {
        self.validate()?;
        let _anchor = crate::files::path::root(&self.root)?;
        let directory = self.root.join(&self.cwd).canonicalize().map_err(|error| {
            Fault::invalid(format!("command working directory is unavailable: {error}"))
        })?;
        if !directory.is_dir() {
            return Err(Fault::invalid(
                "command working directory is not a directory",
            ));
        }
        if self.access == Access::Workspace
            && sandbox::SUPPORTED
            && !directory.starts_with(&self.root)
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "command working directory is outside the workspace",
            ));
        }
        Ok(directory)
    }
}

pub(crate) struct Child {
    pub(crate) inner: Box<dyn ChildWrapper>,
    pub(crate) armed: bool,
}

impl Child {
    pub(crate) async fn wait_root(&mut self) -> std::io::Result<ExitStatus> {
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

#[cfg(test)]
pub(crate) async fn execute(
    launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
) -> Result<Completion, Fault> {
    execute_configured(launch, environment, stop, closed, Streams::default()).await
}

pub(crate) async fn execute_retained(
    launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    retained: Arc<output::Retained>,
) -> Result<Completion, Fault> {
    execute_configured(
        launch,
        environment,
        stop,
        closed,
        Streams {
            retained: Some(retained),
            ..Default::default()
        },
    )
    .await
}

pub(crate) async fn execute_background(
    launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    progress: &tokio::sync::watch::Sender<(Capture, Capture)>,
    input: tokio::sync::mpsc::Receiver<Vec<u8>>,
    retained: Arc<output::Retained>,
) -> Result<Completion, Fault> {
    if launch.pty {
        return interactive::execute(launch, environment, stop, closed, progress, input, retained)
            .await;
    }
    execute_configured(
        launch,
        environment,
        stop,
        closed,
        Streams {
            progress: Some(progress),
            input: Some(input),
            retained: Some(retained),
            ..Default::default()
        },
    )
    .await
}

pub(crate) async fn execute_hook(
    launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    metadata: &std::collections::BTreeMap<&'static str, String>,
) -> Result<Completion, Fault> {
    execute_configured(
        launch,
        environment,
        stop,
        closed,
        Streams {
            metadata: Some(metadata),
            ..Default::default()
        },
    )
    .await
}

#[derive(Default)]
struct Streams<'a> {
    progress: Option<&'a tokio::sync::watch::Sender<(Capture, Capture)>>,
    metadata: Option<&'a std::collections::BTreeMap<&'static str, String>>,
    input: Option<tokio::sync::mpsc::Receiver<Vec<u8>>>,
    retained: Option<Arc<output::Retained>>,
}

async fn execute_configured(
    mut launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    streams: Streams<'_>,
) -> Result<Completion, Fault> {
    let Streams {
        progress,
        metadata,
        mut input,
        retained,
    } = streams;
    let started = Instant::now();
    if stop.is_cancelled() || closed.is_cancelled() {
        return Ok(cancelled());
    }
    let directory = launch.directory()?;
    let inputs = match launch.inputs.take() {
        Some(inputs) => Some(inputs.stage(stop.clone()).await?),
        None => None,
    };
    let sandbox = sandbox::Sandbox::prepare(
        &launch,
        environment,
        inputs.as_ref().map(|directory| directory.path()),
    )?;
    let command = environment.command(
        &launch,
        &directory,
        inputs.as_ref().map(|directory| directory.path()),
    );
    let mut command = match &sandbox {
        Some(sandbox) => sandbox.command(command)?,
        None => command,
    };
    if input.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    if let Some(metadata) = metadata {
        command.envs(metadata);
    }
    let mut command = self::command(command);
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
    #[cfg(target_os = "macos")]
    let group = macos::Group::capture(child.inner.as_ref());
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
    let mut stdin = child.inner.stdin().take();
    let mut capture = output::Drain::with_retained(stdout, stderr, progress.is_some(), retained);
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
                bytes = async { input.as_mut().expect("input receiver is present").recv().await }, if input.is_some() => {
                    let Some(bytes) = bytes else { input = None; stdin.take(); continue; };
                    use tokio::io::AsyncWriteExt;
                    let write = async {
                        let stdin = stdin.as_mut().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
                        stdin.write_all(&bytes).await?;
                        stdin.flush().await
                    };
                    tokio::pin!(write);
                    let written = loop {
                        tokio::select! {
                            biased;
                            _ = closed.cancelled() => break Err(Outcome::Cancelled),
                            _ = stop.cancelled() => break Err(Outcome::Cancelled),
                            result = &mut wait => break Err(match result {
                                Ok(status) => exited(status),
                                Err(error) => Outcome::Unknown(format!("command wait failed: {error}")),
                            }),
                            result = &mut write => break result.map_err(|error| Outcome::Unknown(format!("command input failed: {error}"))),
                            _ = tick.tick(), if progress.is_some() => {
                                progress.unwrap().send_replace(capture.snapshot());
                            },
                        }
                    };
                    if let Err(outcome) = written {
                        break outcome;
                    }
                }
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
        #[cfg(target_os = "macos")]
        let absent = absent
            || error.raw_os_error() == Some(libc::EPERM)
                && group.as_ref().is_some_and(macos::Group::settled);
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
