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
}

impl Process {
    pub fn start(
        root: &Path,
        viewport: &Viewport,
        settings: &sailry_protocol::terminal::Settings,
        tool: Option<sailry_protocol::terminal::Tool>,
        messages: SyncSender<Message>,
    ) -> Result<Self, Fault> {
        let pair = portable_pty::native_pty_system()
            .openpty(size(viewport))
            .map_err(failure)?;
        let mut command = if let Some(tool) = tool {
            CommandBuilder::new(super::super::tools::resolve(root, settings, tool)?)
        } else {
            shell(&settings.shell)?
        };
        for (key, value) in &settings.environment {
            command.env(key, value);
        }
        #[cfg(unix)]
        let integration = if tool.is_none() {
            integration::configure(&mut command)?
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

fn shell(configured: &str) -> Result<CommandBuilder, Fault> {
    #[cfg(unix)]
    {
        login::command(configured)
    }
    #[cfg(windows)]
    {
        Ok(if configured.is_empty() {
            CommandBuilder::new_default_prog()
        } else {
            CommandBuilder::new(configured)
        })
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
