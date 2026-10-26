use sailry_protocol::{Command, TerminalId, terminal::Viewport};
use std::{collections::VecDeque, sync::Mutex};
use tokio::sync::Notify;

const CAPACITY: usize = 64;

#[derive(Clone, Eq, PartialEq)]
struct Resize {
    terminal: TerminalId,
    revision: u64,
    viewport: Viewport,
}

impl Resize {
    fn from(command: &Command) -> Option<Self> {
        match command {
            Command::ResizeTerminal {
                terminal,
                revision,
                viewport,
            } => Some(Self {
                terminal: *terminal,
                revision: *revision,
                viewport: viewport.clone(),
            }),
            _ => None,
        }
    }

    fn matches(&self, command: &Command) -> bool {
        matches!(command, Command::ResizeTerminal { terminal, revision, .. }
            if *terminal == self.terminal && *revision == self.revision)
    }
}

#[derive(Default)]
struct State {
    pending: VecDeque<Command>,
    in_flight: Option<Resize>,
    acknowledged: Option<Resize>,
    closed: bool,
}

impl State {
    fn push(&mut self, command: Command) -> bool {
        if self.closed {
            return false;
        }
        if let Some(resize) = Resize::from(&command) {
            if let Some(pending) = self
                .pending
                .iter_mut()
                .find(|pending| resize.matches(pending))
            {
                *pending = command;
                return true;
            }
            if self.in_flight.as_ref() == Some(&resize)
                || self.in_flight.is_none() && self.acknowledged.as_ref() == Some(&resize)
            {
                return true;
            }
        }
        if self.pending.len() == CAPACITY {
            return false;
        }
        self.pending.push_back(command);
        true
    }

    fn pop(&mut self) -> Option<Command> {
        let command = self.pending.pop_front()?;
        self.in_flight = Resize::from(&command);
        Some(command)
    }

    fn complete(&mut self, success: bool) {
        if self.closed {
            self.discard();
        } else if !success {
            // Later geometry is a separate, idempotent request. Byte input must
            // still be discarded after a failed command rather than replayed.
            let resize = self.in_flight.as_ref().and_then(|current| {
                self.pending
                    .iter()
                    .rev()
                    .find(|command| {
                        matches!(command, Command::ResizeTerminal { terminal, .. }
                        if *terminal == current.terminal)
                    })
                    .cloned()
            });
            self.discard();
            if let Some(resize) = resize {
                self.pending.push_back(resize);
            }
        } else if let Some(resize) = self.in_flight.take() {
            self.acknowledged = Some(resize);
        }
    }

    fn discard(&mut self) {
        self.pending.clear();
        self.in_flight = None;
        self.acknowledged = None;
    }
}

#[derive(Default)]
pub(super) struct Queue {
    state: Mutex<State>,
    ready: Notify,
}

impl Queue {
    pub fn enqueue(&self, command: Command) -> bool {
        let accepted = self.state.lock().unwrap().push(command);
        if accepted {
            self.ready.notify_one();
        }
        accepted
    }

    pub async fn next(&self) -> Command {
        loop {
            let notified = self.ready.notified();
            if let Some(command) = self.state.lock().unwrap().pop() {
                return command;
            }
            notified.await;
        }
    }

    pub fn complete(&self, success: bool) {
        self.state.lock().unwrap().complete(success);
    }

    pub fn reset_resize(&self) {
        self.state.lock().unwrap().acknowledged = None;
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.discard();
    }
}

#[cfg(test)]
mod tests;
