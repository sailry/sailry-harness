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
        let closed = stop.clone();
        binding.runtime.spawn(async move {
            loop {
                let command = tokio::select! {
                    biased;
                    _ = closed.cancelled() => break,
                    command = commands.recv() => match command { Some(command) => command, None => break },
                };
                let report = !matches!(command, Command::InputTerminal { .. });
                let mut request = client.prepare(command);
                if let Some(scope) = &scope { request.plugin = Some(scope.context.clone()); }
                let result = tokio::select! {
                    _ = closed.cancelled() => break,
                    result = client.execute(request) => result.map(|_| ()),
                };
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
        self.input.try_send(command).is_ok()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
