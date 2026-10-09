use super::super::{failure, session::Message};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use sailry_protocol::{Fault, terminal::Viewport};
use std::{path::Path, sync::mpsc::SyncSender};

#[cfg(unix)]
#[path = "integration.rs"]
mod integration;
#[cfg(unix)]
#[path = "login.rs"]
mod login;

#[cfg(unix)]
#[path = "unix.rs"]
mod io;
#[cfg(windows)]
#[path = "windows.rs"]
mod io;

pub(in crate::terminal) struct Process {
    pub child: Box<dyn Child + Send + Sync>,
    master: Option<Box<dyn MasterPty + Send>>,
    io: io::Io,
    _integration: Option<tempfile::TempDir>,
    _sandbox: Option<crate::process::sandbox::Sandbox>,
}

impl Process {
    pub fn start(
        source: super::Source,
        viewport: &Viewport,
        messages: SyncSender<Message>,
    ) -> Result<Self, Fault> {
        let super::Source::Local {
            root,
            profile,
            access,
            settings,
            tool,
            environment,
        } = source
        else {
            unreachable!("local terminal source expected")
        };
        let root = root.as_path();
        let profile = profile.as_deref();
        let pair = portable_pty::native_pty_system()
            .openpty(size(viewport))
            .map_err(failure)?;
        let mut command = if let Some(tool) = tool {
            CommandBuilder::new(super::super::tools::resolve(
                root,
                &settings,
                tool,
                &environment,
            )?)
        } else {
            CommandBuilder::new_default_prog()
        };
        environment.apply_terminal(&mut command);
        for (key, value) in &settings.environment {
            command.env(key, value);
        }
        if tool.is_none() {
            shell(&settings.shell, root, &mut command)?;
        }
        let scratch = if access == sailry_protocol::process::Access::Workspace
            && crate::process::sandbox::SUPPORTED
        {
            let directory = if let Some(profile) = profile {
                let directory = profile.join("tmp");
                std::fs::create_dir_all(&directory).map_err(failure)?;
                tempfile::Builder::new()
                    .prefix("terminal-")
                    .tempdir_in(directory)
                    .map_err(failure)?
            } else {
                tempfile::Builder::new()
                    .prefix("sailry-terminal-")
                    .tempdir()
                    .map_err(failure)?
            };
            Some(std::sync::Arc::new(directory))
        } else {
            None
        };
        let sandbox = crate::process::sandbox::Sandbox::prepare(
            &crate::process::Launch {
                root: root.to_owned(),
                profile: profile.map(Path::to_owned),
                command: "terminal".into(),
                access,
                cwd: String::new(),
                timeout_ms: 900_000,
                inputs: None,
                scratch,
            },
            &environment,
            None,
        )?;
        if let Some(sandbox) = &sandbox {
            sandbox.apply_terminal(&mut command);
        }
        #[cfg(unix)]
        let integration = if tool.is_none() {
            integration::configure(
                &mut command,
                sandbox.as_ref().map(|sandbox| sandbox.scratch()),
            )?
        } else {
            None
        };
        #[cfg(not(unix))]
        let integration = None;
        command.cwd(root);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "ghostty");
        command.env(
            "TERM_PROGRAM_VERSION",
            libghostty_vt::build_info::version_string().map_err(failure)?,
        );
        command.env_remove("VTE_VERSION");
        command.env("PWD", root);
        if let Some(sandbox) = &sandbox {
            #[cfg(unix)]
            let device = pair.master.tty_name();
            #[cfg(not(unix))]
            let device: Option<std::path::PathBuf> = None;
            sandbox.terminal(&mut command, device.as_deref())?;
        }
        let mut child = pair.slave.spawn_command(command).map_err(failure)?;
        drop(pair.slave);
        let io = match io::Io::start(&*pair.master, messages) {
            Ok(io) => io,
            Err(error) => {
                let _ = child.kill();
                drop(pair.master);
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child,
            master: Some(pair.master),
            io,
            _integration: integration,
            _sandbox: sandbox,
        })
    }

    pub fn resize(&self, viewport: &Viewport) -> Result<(), Fault> {
        self.master
            .as_ref()
            .unwrap()
            .resize(size(viewport))
            .map_err(failure)
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), Fault> {
        self.io.write(bytes)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // Release every PTY descriptor before waiting: a foreground TUI can
        // remain in terminal teardown until the master is closed on macOS.
        self.io.stop();
        // Use the retained child handle; no recycled PID or global process search.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            drop(self.master.take());
            let _ = self.child.wait();
        }
    }
}

fn shell(configured: &str, root: &Path, command: &mut CommandBuilder) -> Result<(), Fault> {
    #[cfg(unix)]
    {
        login::configure(configured, root, command)
    }
    #[cfg(windows)]
    {
        let _ = root;
        if !configured.is_empty() {
            command.get_argv_mut().push(configured.into());
        }
        Ok(())
    }
}

fn size(viewport: &Viewport) -> PtySize {
    PtySize {
        cols: viewport.columns,
        rows: viewport.rows,
        pixel_width: viewport.pixel_width.min(u16::MAX.into()) as u16,
        pixel_height: viewport.pixel_height.min(u16::MAX.into()) as u16,
    }
}
