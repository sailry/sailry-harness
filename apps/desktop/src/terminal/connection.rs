use sailry_client::{Client, terminal::View};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Fault, NodeId, TerminalId,
    terminal::{Appearance, Viewport},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Notify, mpsc, watch};

#[derive(Clone)]
pub(crate) struct Scope {
    pub context: sailry_protocol::plugin::Context,
    pub stop: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub client: Arc<Client>,
    pub caller: NodeId,
    pub id: TerminalId,
    pub runtime: tokio::runtime::Handle,
    /// Scoped mounts attach to an explicitly opened PTY; releasing them only stops observation.
    pub scope: Option<Scope>,
}

pub(super) struct Connection {
    input: mpsc::Sender<Command>,
    stop: CancellationToken,
    retry: Arc<Notify>,
    waiting: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}

impl Connection {
    pub fn start(
        binding: &Binding,
        appearance: Appearance,
    ) -> (
        Self,
        watch::Receiver<View>,
        mpsc::Receiver<Result<(), Fault>>,
    ) {
        let stop = binding
            .scope
            .as_ref()
            .map_or_else(CancellationToken::new, |scope| scope.stop.child_token());
        let scope = binding.scope.clone();
        let (screens, view) = watch::channel(View::default());
        let closed = Arc::new(AtomicBool::new(false));
        let mut lifecycle = view.clone();
        let lifecycle_stop = stop.clone();
        let ended = closed.clone();
        binding.runtime.spawn(async move {
            loop {
                if lifecycle
                    .borrow()
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot.info.status == sailry_protocol::terminal::Status::Closed
                    })
                {
                    ended.store(true, Ordering::Release);
                    lifecycle_stop.cancel();
                    return;
                }
                tokio::select! {
                    _ = lifecycle_stop.cancelled() => return,
                    changed = lifecycle.changed() => if changed.is_err() { return; },
                }
            }
        });
        let client = binding.client.clone();
        let id = binding.id;
        let watch_stop = stop.clone();
        let retry = Arc::new(Notify::new());
        let waiting = Arc::new(AtomicBool::new(false));
        let reopen = retry.clone();
        let retrying = waiting.clone();
        binding.runtime.spawn(async move {
            // Opening is an explicit durable command; screen recovery stays read-only.
            let command = Command::OpenTerminal {
                terminal: id,
                viewport: Viewport {
                    columns: 80,
                    rows: 24,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                appearance,
            };
            let mut request = scope.is_none().then(|| client.prepare(command.clone()));
            loop {
                let result = tokio::select! {
                    _ = watch_stop.cancelled() => return,
                    result = async {
                        if let Some(request) = &request { client.execute(request.clone()).await?; }
                        Ok(())
                    } => result,
                };
                let error = match result {
                    Ok(_) => match client
                        .watch_terminal(id, screens.clone(), watch_stop.clone())
                        .await
                    {
                        Ok(()) => return,
                        Err(error) => error,
                    },
                    Err(error) => error,
                };
                if request.is_some() && error.code != sailry_protocol::ErrorCode::OutcomeUnknown {
                    request = Some(client.prepare(command.clone()));
                }
                retrying.store(true, Ordering::Release);
                screens.send_modify(|view| {
                    view.connected = false;
                    view.error = Some(error);
                });
                tokio::select! {
                    _ = watch_stop.cancelled() => return,
                    _ = reopen.notified() => {},
                }
                screens.send_modify(|view| view.error = None);
            }
        });
        let (input, mut commands) = mpsc::channel::<Command>(64);
        let (completed, results) = mpsc::channel(16);
        let client = binding.client.clone();
        let scope = binding.scope.clone();
        let input_stop = stop.clone();
        let ended = closed.clone();
        binding.runtime.spawn(async move {
            loop {
                let command = tokio::select! {
                    biased;
                    _ = input_stop.cancelled() => break,
                    command = commands.recv() => match command { Some(command) => command, None => break },
                };
                let report = !matches!(command, Command::InputTerminal { .. });
                let mut request = client.prepare(command);
                // Byte input is owned by this native renderer; SDK requests retain their scope.
                if !matches!(request.command, Command::InputTerminal { .. })
                    && let Some(scope) = &scope {
                    request.plugin = Some(scope.context.clone());
                }
                let result = tokio::select! {
                    _ = input_stop.cancelled() => break,
                    result = client.execute(request) => result.map(|_| ()),
                };
                if result.as_ref().is_err_and(|error| matches!(error.code,
                    sailry_protocol::ErrorCode::NotFound | sailry_protocol::ErrorCode::Conflict | sailry_protocol::ErrorCode::Unavailable)) {
                    let confirmation = tokio::select! {
                        _ = input_stop.cancelled() => break,
                        result = client.execute(client.prepare(Command::Snapshot)) => result,
                    };
                    if confirmation.is_ok_and(|output| matches!(output, sailry_protocol::Output::Snapshot(snapshot)
                        if snapshot.terminals.iter().any(|info| info.id == id && info.status == sailry_protocol::terminal::Status::Closed))) {
                        ended.store(true, Ordering::Release);
                        input_stop.cancel();
                        break;
                    }
                }
                if result.is_err() {
                    // Do not deliver a queued tail after a disconnect or ownership conflict.
                    while commands.try_recv().is_ok() {}
                }
                if (report || result.is_err()) && completed.send(result).await.is_err() { break; }
            }
        });
        (
            Self {
                input,
                stop,
                retry,
                waiting,
                closed,
            },
            view,
            results,
        )
    }

    pub fn can_retry(&self) -> bool {
        self.waiting.load(Ordering::Acquire)
    }

    pub fn retry(&self) {
        if self.waiting.swap(false, Ordering::AcqRel) {
            self.retry.notify_one();
        }
    }

    pub fn enqueue(&self, command: Command) -> bool {
        !self.closed() && !self.stopped() && self.input.try_send(command).is_ok()
    }

    pub fn stopped(&self) -> bool {
        self.stop.is_cancelled()
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn release(&self) {
        self.stop.cancel();
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests;
