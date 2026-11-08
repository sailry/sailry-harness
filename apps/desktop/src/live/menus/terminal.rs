//! Sidebar resource closure does not require a mounted terminal renderer.
use super::*;
use sailry_protocol::{ErrorCode, Request, Snapshot, terminal::Status};

pub(super) fn items(pinned: bool) -> Vec<(Command, &'static str)> {
    vec![
        (Command::Open, "workspace_open_terminal"),
        if pinned {
            (Command::Unpin, "workspace_unpin")
        } else {
            (Command::Pin, "workspace_pin")
        },
        (Command::Copy, "workspace_copy_path"),
        (Command::Close, "workspace_close_terminal"),
    ]
}

impl Dispatch {
    pub(super) fn terminal_pin(&self, snapshot: &Snapshot) -> Option<(TerminalId, bool)> {
        let pinned = match self.command {
            Command::Pin => true,
            Command::Unpin => false,
            _ => return None,
        };
        let Target::Terminal(id) = self.target else {
            return None;
        };
        if self.node != snapshot.node {
            return None;
        }
        let terminal = snapshot
            .terminals
            .iter()
            .find(|info| info.id == id && info.status != Status::Closed)?;
        let worktree = terminal.worktree?;
        snapshot.worktrees.iter().find(|tree| tree.id == worktree)?;
        Some((id, pinned))
    }

    pub(super) fn terminal_close(&self, snapshot: &Snapshot) -> Option<sailry_protocol::Command> {
        let Target::Terminal(id) = self.target else {
            return None;
        };
        if self.command != Command::Close || self.node != snapshot.node {
            return None;
        }
        let terminal = snapshot
            .terminals
            .iter()
            .find(|info| info.id == id && info.status != Status::Closed)?;
        let worktree = terminal.worktree?;
        snapshot.worktrees.iter().find(|tree| tree.id == worktree)?;
        Some(sailry_protocol::Command::CloseTerminal {
            worktree,
            terminal: id,
        })
    }
}

impl Shell {
    pub(super) fn close_live_terminal(
        &mut self,
        command: sailry_protocol::Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        let sailry_protocol::Command::CloseTerminal { worktree, terminal } = command else {
            return;
        };
        let client = live.client();
        let node = client.target();
        let key = (node, terminal);
        let closing = self
            .terminals
            .closing
            .entry(key)
            .or_insert_with(|| Closing {
                request: client.prepare(command),
                pending: false,
            });
        if closing.pending {
            return;
        }
        closing.pending = true;
        let request = closing.request.clone();
        let job = cx
            .global::<Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                let error = match result {
                    Ok(Ok(Output::Terminal(info)))
                        if info.id == terminal && info.status == Status::Closed =>
                    {
                        None
                    }
                    Ok(Err(error)) if error.code == ErrorCode::NotFound => None,
                    Ok(Err(error))
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                        ) =>
                    {
                        Some(("live_request_failed", false))
                    }
                    _ => Some(("chat_action_unknown", true)),
                };
                if let Some((message, uncertain)) = error {
                    if uncertain {
                        if let Some(closing) = shell.terminals.closing.get_mut(&key) {
                            closing.pending = false;
                        }
                    } else {
                        shell.terminals.closing.remove(&key);
                    }
                    crate::feedback::toast(
                        window,
                        tr(message),
                        Notification::error(tr(message)),
                        cx,
                    );
                } else {
                    shell.terminals.closing.remove(&key);
                    shell.remove_pane(
                        crate::panes::Target::Terminal(node, worktree, terminal),
                        window,
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }
}

pub(crate) struct Closing {
    request: Request,
    pending: bool,
}

#[cfg(test)]
mod tests;
