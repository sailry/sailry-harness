//! PTY operations share the captured worktree and original durable request IDs.
use super::*;
use sailry_protocol::{TerminalId, WorktreeId, terminal};

impl Host {
    pub(super) fn terminal_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let list = self.clone();
        let tools = self.clone();
        let create = self.clone();
        let open = self.clone();
        let close = self.clone();
        module
            .async_function("listTerminals", move |_| {
                list.read_public(Command::ListTerminals {
                    worktree: list.terminal_worktree()?,
                })
            })
            .async_function("listTerminalTools", move |_| {
                tools.read_public(Command::ListTerminalTools {
                    worktree: tools.terminal_worktree()?,
                })
            })
            .function("prepareTerminal", move |args| {
                let launch = terminal::Launch {
                    worktree: create.terminal_worktree()?,
                    viewport: viewport(),
                    appearance: appearance()?,
                };
                let command = if let Some(tool) = args
                    .get(0)
                    .filter(|value| !matches!(value, HostValue::Null))
                {
                    Command::OpenToolTerminal {
                        tool: serde_json::from_value(decode(tool)?)
                            .map_err(|_| HostError::new("invalid terminal tool"))?,
                        launch,
                    }
                } else {
                    Command::CreateTerminal(launch)
                };
                create.prepare_public(command)
            })
            .function("prepareOpenTerminal", move |args| {
                open.prepare_public(Command::OpenTerminal {
                    terminal: identity(args.string(0)?)?,
                    viewport: viewport(),
                    appearance: appearance()?,
                })
            })
            .function("prepareCloseTerminal", move |args| {
                close.prepare_public(Command::CloseTerminal {
                    worktree: close.terminal_worktree()?,
                    terminal: identity(args.string(0)?)?,
                })
            })
    }

    fn terminal_worktree(&self) -> Result<WorktreeId, HostError> {
        self.check()?;
        self.context
            .worktree
            .ok_or_else(|| HostError::new("plugin has no worktree scope"))
    }
}

fn identity(value: &str) -> Result<TerminalId, HostError> {
    value
        .parse()
        .map_err(|_| HostError::new("invalid terminal ID"))
}

fn viewport() -> terminal::Viewport {
    terminal::Viewport {
        columns: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn appearance() -> Result<terminal::Appearance, HostError> {
    gpui_shell::with_current_app(|cx| crate::theme::terminal(cx))
        .ok_or_else(|| HostError::new("terminal creation requires an active view"))
}
