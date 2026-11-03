use sailry_protocol::{
    Command, TerminalId,
    terminal::{MAX_INPUT_BATCH, MAX_INPUT_BYTES, Viewport},
};
use std::{collections::VecDeque, sync::Mutex};
use tokio::sync::Notify;
use tokio::time::{Duration, Instant};

/// One round trip can drain the whole queue as a single input batch.
const CAPACITY: usize = MAX_INPUT_BATCH;
/// Pointer motion and wheel ticks are best-effort: behind this backlog they
/// are dropped instead of failing input, keeping room for typed keys.
pub(super) const POINTER_BACKLOG: usize = CAPACITY / 2;
const RETRY_DELAY: Duration = Duration::from_millis(250);

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

fn pointer(command: &Command) -> bool {
    use sailry_protocol::terminal::{Input, MouseAction, MouseButton};
    let Command::InputTerminal { inputs, .. } = command else {
        return false;
    };
    inputs.iter().all(|input| {
        matches!(input, Input::Mouse { event }
        if event.action == MouseAction::Motion
            || matches!(event.button, Some(MouseButton::WheelUp | MouseButton::WheelDown
                | MouseButton::WheelLeft | MouseButton::WheelRight)))
    })
}

/// Appends queued input for the same terminal revision to `batch`, preserving order.
fn absorb(batch: &mut Command, next: &Command) -> bool {
    let (
        Command::InputTerminal {
            terminal,
            revision,
            inputs,
        },
        Command::InputTerminal {
            terminal: other,
            revision: version,
            inputs: tail,
        },
    ) = (batch, next)
    else {
        return false;
    };
    let bytes = |inputs: &[sailry_protocol::terminal::Input]| {
        inputs.iter().map(|input| input.text_bytes()).sum::<usize>()
    };
    if terminal != other
        || revision != version
        || inputs.len() + tail.len() > MAX_INPUT_BATCH
        || bytes(inputs) + bytes(tail) > MAX_INPUT_BYTES
    {
        return false;
    }
    inputs.extend(tail.iter().cloned());
    true
}

#[derive(Default)]
struct State {
    pending: VecDeque<Command>,
    in_flight: Option<Resize>,
    acknowledged: Option<Resize>,
    retry_after: Option<Instant>,
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
        if pointer(&command) && self.pending.len() >= POINTER_BACKLOG {
            return true;
        }
        if self.pending.len() == CAPACITY {
            return false;
        }
        self.pending.push_back(command);
        true
    }

    fn pop(&mut self) -> Option<Command> {
        self.pop_at(Instant::now())
    }

    fn pop_at(&mut self, now: Instant) -> Option<Command> {
        if self.retry_after.is_some_and(|deadline| now < deadline)
            && self.pending.front().and_then(Resize::from).is_some()
        {
            return None;
        }
        let mut command = self.pending.pop_front()?;
        while let Some(next) = self.pending.front()
            && absorb(&mut command, next)
        {
            self.pending.pop_front();
        }
        self.in_flight = Resize::from(&command);
        Some(command)
    }

    fn complete(&mut self, success: bool) {
        if self.closed {
            self.discard();
        } else if !success {
            // Later geometry is a separate, idempotent request. Byte input must
            // still be discarded after a failed command rather than replayed.
            let retry = self.in_flight.is_some();
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
            self.retry_after = retry.then(|| Instant::now() + RETRY_DELAY);
            if let Some(resize) = resize {
                self.pending.push_back(resize);
            }
        } else if let Some(resize) = self.in_flight.take() {
            self.acknowledged = Some(resize);
            self.retry_after = None;
        }
    }

    fn discard(&mut self) {
        self.pending.clear();
        self.in_flight = None;
        self.acknowledged = None;
        self.retry_after = None;
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
            let deadline = {
                let mut state = self.state.lock().unwrap();
                if let Some(command) = state.pop() {
                    return command;
                }
                state.retry_after.filter(|_| !state.pending.is_empty())
            };
            if let Some(deadline) = deadline {
                tokio::select! {
                    _ = notified => {},
                    _ = tokio::time::sleep_until(deadline) => {},
                }
            } else {
                notified.await;
            }
        }
    }

    pub fn backlog(&self) -> usize {
        self.state.lock().unwrap().pending.len()
    }

    pub fn complete(&self, success: bool) {
        self.state.lock().unwrap().complete(success);
    }

    pub fn reset_resize(&self) {
        let mut state = self.state.lock().unwrap();
        state.acknowledged = None;
        state.retry_after = None;
        self.ready.notify_one();
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.closed = true;
        state.discard();
    }
}

#[cfg(test)]
mod tests;
