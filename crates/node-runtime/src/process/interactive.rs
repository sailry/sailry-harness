//! PTY commands keep the managed-command scope and bounded output lifecycle.
use super::{Environment, Launch, output::Buffer, pty, sandbox};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    process::{Capture, Completion, Outcome},
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
pub(super) mod windows;

struct Process {
    child: Box<dyn Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    io: Option<pty::Io>,
    #[cfg(windows)]
    job: Option<windows::Job>,
    _launch: Launch,
    _sandbox: Option<sandbox::Sandbox>,
    _inputs: Option<tempfile::TempDir>,
    armed: bool,
}

impl Process {
    fn terminate(&mut self) -> Result<(), Fault> {
        if !self.armed {
            return Ok(());
        }
        #[cfg(unix)]
        if let Some(pid) = self.child.process_id() {
            // SAFETY: portable-pty creates a session whose process-group leader is
            // this retained child. The negative PID targets only that owned group.
            let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(failure(error));
                }
            }
        }
        #[cfg(windows)]
        self.job.take();
        if !matches!(self.child.try_wait(), Ok(Some(_)))
            && let Err(error) = self.child.kill()
        {
            #[cfg(unix)]
            let absent = error.raw_os_error() == Some(libc::ESRCH);
            #[cfg(not(unix))]
            let absent = false;
            if !absent {
                return Err(failure(error));
            }
        }
        self.armed = false;
        Ok(())
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.terminate();
        #[cfg(windows)]
        {
            let master = self.master.take();
            let io = self.io.take();
            if master.is_some() {
                // The reader must stay alive through ClosePseudoConsole, including
                // when the execution future is dropped before normal cleanup.
                drop(tokio::task::spawn_blocking(move || {
                    drop(master);
                    drop(io);
                }));
            } else {
                drop(io);
            }
        }
        #[cfg(not(windows))]
        {
            self.io.take();
            self.master.take();
        }
    }
}

pub(super) async fn execute(
    mut launch: Launch,
    environment: &Environment,
    stop: &CancellationToken,
    closed: &CancellationToken,
    progress: &tokio::sync::watch::Sender<(Capture, Capture)>,
    mut input: tokio::sync::mpsc::Receiver<Vec<u8>>,
    retained: Arc<super::output::Retained>,
) -> Result<Completion, Fault> {
    let started = Instant::now();
    if stop.is_cancelled() || closed.is_cancelled() {
        return Ok(super::cancelled());
    }
    let directory = launch.directory()?;
    let inputs = match launch.inputs.take() {
        Some(inputs) => Some(inputs.stage(stop.clone()).await?),
        None => None,
    };
    let environment = environment.clone();
    let buffer = Arc::new(Mutex::new(Buffer::tail()));
    let output = buffer.clone();
    let error = Arc::new(Mutex::new(None));
    let read_error = error.clone();
    #[cfg(windows)]
    let input_error = Arc::new(Mutex::new(None));
    #[cfg(windows)]
    let write_error = input_error.clone();
    let ready = progress.clone();
    let drained = Arc::new(AtomicBool::new(false));
    let finished = drained.clone();
    let sink = Arc::new(move |event| {
        let mut output = output.lock().unwrap();
        match event {
            pty::Event::Bytes(bytes) => {
                output.append(&bytes);
                retained.append(sailry_protocol::process::OutputStream::Stdout, &bytes);
            }
            pty::Event::Eof(result) => {
                output.finish();
                if let Err(error) = result {
                    *read_error.lock().unwrap() = Some(error);
                }
                finished.store(true, Ordering::Release);
            }
            #[cfg(windows)]
            pty::Event::InputError(error) => {
                *write_error.lock().unwrap() = Some(error);
            }
        }
        ready.send_replace((output.capture(), Capture::default()));
        true
    });
    let mut process = tokio::task::spawn_blocking(move || {
        let sandbox = sandbox::Sandbox::prepare(
            &launch,
            &environment,
            inputs.as_ref().map(|inputs| inputs.path()),
        )?;
        #[cfg(windows)]
        let system: Box<dyn portable_pty::PtySystem + Send> =
            Box::new(portable_pty::NativePtySystem::default().inherit_cursor(false));
        #[cfg(not(windows))]
        let system = portable_pty::native_pty_system();
        let pair = system
            .openpty(PtySize {
                rows: 24,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(failure)?;
        let mut command = CommandBuilder::new(environment.shell());
        #[cfg(unix)]
        command.args(["-c", &launch.command]);
        #[cfg(windows)]
        command.args(["/D", "/S", "/C", &launch.command]);
        environment.apply_pty(&mut command);
        if let Some(python) = &launch.python {
            for (key, value) in python.variables() {
                command.env(key, value);
            }
        }
        command.cwd(&directory);
        command.env("PWD", &directory);
        command.env("TERM", "xterm-256color");
        if let Some(inputs) = &inputs {
            command.env("SAILRY_ATTACHMENTS", inputs.path());
        }
        if let Some(scratch) = &launch.scratch {
            for key in ["TMPDIR", "TMP", "TEMP"] {
                command.env(key, scratch.path());
            }
        }
        if let Some(sandbox) = &sandbox {
            sandbox.apply_terminal(&mut command);
            #[cfg(unix)]
            let device = pair.master.tty_name();
            #[cfg(not(unix))]
            let device: Option<std::path::PathBuf> = None;
            sandbox.terminal(&mut command, device.as_deref())?;
        }
        #[cfg(windows)]
        let job = windows::Job::new()?;
        #[cfg(windows)]
        job.configure(&mut command);
        let child = pair.slave.spawn_command(command).map_err(failure)?;
        drop(pair.slave);
        let mut process = Process {
            child,
            master: Some(pair.master),
            io: None,
            #[cfg(windows)]
            job: Some(job),
            _launch: launch,
            _sandbox: sandbox,
            _inputs: inputs,
            armed: true,
        };
        process.io = Some(pty::Io::start(process.master.as_deref().unwrap(), sink)?);
        Ok::<_, Fault>(process)
    })
    .await
    .map_err(|_| failure("command PTY launch failed"))??;
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    let mut input_open = true;
    let mut pending = None;
    let mut outcome = loop {
        tokio::select! {
            biased;
            _ = closed.cancelled() => break Outcome::Cancelled,
            _ = stop.cancelled() => break Outcome::Cancelled,
            bytes = input.recv(), if input_open && pending.is_none() => {
                match bytes {
                    Some(bytes) => pending = Some(bytes),
                    None => input_open = false,
                }
            }
            _ = tick.tick() => {
                match process.child.try_wait() {
                    Ok(Some(status)) => break match status.signal() {
                        Some(signal) => Outcome::Unknown(format!("command terminated by {signal}; numeric signal is unavailable")),
                        None => Outcome::Exited(status.exit_code() as i32),
                    },
                    Ok(None) => {},
                    Err(error) => break Outcome::Unknown(format!("command PTY wait failed: {error}")),
                }
                if let Some(error) = error.lock().unwrap().take() { break Outcome::Unknown(error); }
                #[cfg(windows)]
                if let Some(error) = input_error.lock().unwrap().take() { break Outcome::Unknown(error); }
                if let Some(bytes) = &pending {
                    match process.io.as_ref().unwrap().write(bytes.clone()) {
                        Ok(()) => pending = None,
                        Err(error) if error.code == ErrorCode::Busy => {},
                        Err(error) => break Outcome::Unknown(error.message),
                    }
                }
            }
        }
    };
    if let Err(error) = process.terminate() {
        outcome = Outcome::Unknown(error.message);
    }
    let deadline = tokio::time::Instant::now() + super::CLEANUP_TIMEOUT;
    loop {
        match process.child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
            Ok(None) => {
                outcome = Outcome::Unknown("command PTY cleanup timed out".into());
                break;
            }
            Err(error) => {
                outcome = Outcome::Unknown(format!("command PTY reap failed: {error}"));
                break;
            }
        }
    }
    #[cfg(windows)]
    {
        let master = process.master.take();
        let io = process.io.take();
        // Closing can emit a final frame and block on earlier Windows versions.
        // Keep the reader inside the task so dropping this future cannot stop it.
        process.io = tokio::task::spawn_blocking(move || {
            drop(master);
            io
        })
        .await
        .map_err(|error| failure(format!("command PTY close failed: {error}")))?;
    }
    #[cfg(windows)]
    let deadline = tokio::time::Instant::now() + super::CLEANUP_TIMEOUT;
    while !drained.load(Ordering::Acquire) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if !drained.load(Ordering::Acquire) {
        outcome = Outcome::Unknown("command PTY output drain timed out".into());
    }
    if let Some(error) = error.lock().unwrap().take() {
        outcome = Outcome::Unknown(error);
    }
    process.io.as_mut().unwrap().stop();
    buffer.lock().unwrap().finish();
    let stdout = buffer.lock().unwrap().capture();
    Ok(Completion {
        outcome,
        stdout,
        stderr: Capture::default(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("command PTY failed: {error}"),
    )
}

#[cfg(test)]
mod tests;
