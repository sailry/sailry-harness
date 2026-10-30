//! Anonymous counters are aggregated locally and flushed every fifteen minutes.
use super::Control;
use chrono::Utc;
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{
    sync::{Notify, mpsc, oneshot},
    time::{Instant, MissedTickBehavior},
};

mod aggregate;
mod capture;
#[cfg(any(test, feature = "test-support"))]
pub(super) mod fixture;
mod transport;
mod types;
pub(crate) use capture::{Call, Request, Search};
pub(crate) use types::{
    Model, Observation, RequestResult, StatusClass, Tokens, Tool, ToolResult, TurnResult,
};

const QUEUE_LIMIT: usize = 1024;
const PERIOD: Duration = Duration::from_secs(15 * 60);

pub(crate) struct Metrics {
    control: Arc<Control>,
    records: mpsc::Sender<Envelope>,
    clear: Arc<Notify>,
    shutdown: mpsc::Sender<Shutdown>,
}

struct Envelope {
    epoch: u64,
    observation: Observation,
}
struct Shutdown {
    deadline: Instant,
    reply: oneshot::Sender<()>,
}

struct Pending {
    epoch: u64,
    batches: VecDeque<aggregate::Batch>,
}

impl From<aggregate::Window> for Pending {
    fn from(window: aggregate::Window) -> Self {
        Self {
            epoch: window.epoch(),
            batches: window.finish(Utc::now()).into(),
        }
    }
}

impl Metrics {
    pub(crate) fn new(control: Arc<Control>, key: Option<String>) -> Self {
        let (records, received) = mpsc::channel(QUEUE_LIMIT);
        let (shutdown, closing) = mpsc::channel(1);
        let clear = Arc::new(Notify::new());
        let worker_control = control.clone();
        let worker_clear = clear.clone();
        let _ = std::thread::Builder::new()
            .name("sailry-metrics".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let Some(transport) = key.as_deref().and_then(transport::Transport::new) else {
                        return;
                    };
                    worker(worker_control, received, closing, worker_clear, transport).await;
                });
            });
        Self {
            control,
            records,
            clear,
            shutdown,
        }
    }

    pub(crate) fn record(&self, observation: Observation) {
        self.record_at(self.control.epoch(), observation);
    }

    pub(crate) fn record_at(&self, epoch: u64, observation: Observation) {
        if !enabled(&self.control) || self.control.epoch() != epoch {
            return;
        }
        let _ = self.records.try_send(Envelope { epoch, observation });
    }

    pub(crate) fn clear(&self) {
        self.clear.notify_one();
    }

    pub(crate) async fn shutdown(&self, deadline: Instant) {
        let (reply, received) = oneshot::channel();
        if self.shutdown.try_send(Shutdown { deadline, reply }).is_ok() {
            let _ = tokio::time::timeout_at(deadline, received).await;
        }
    }
}

async fn worker(
    control: Arc<Control>,
    mut records: mpsc::Receiver<Envelope>,
    mut shutdown: mpsc::Receiver<Shutdown>,
    clear: Arc<Notify>,
    transport: transport::Transport,
) {
    let mut interval = tokio::time::interval_at(Instant::now() + PERIOD, PERIOD);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut window = None;
    loop {
        let epoch = control.epoch();
        tokio::select! {
            biased;
            closing = shutdown.recv() => {
                if let Some(closing) = closing {
                    drain(&control, &mut window, &mut records);
                    flush(&control, window.take(), &transport, &clear, Some(closing.deadline)).await;
                    let _ = closing.reply.send(());
                }
                return;
            }
            _ = clear.notified() => { window = None; }
            _ = control.changed(epoch) => { window = None; }
            _ = interval.tick() => {
                let mut pending=window.take().map(Pending::from);
                tokio::select! {
                    biased;
                    closing = shutdown.recv() => {
                        if let Some(closing) = closing {
                            drain(&control, &mut window, &mut records);
                            deliver(&control,&mut pending,&transport,&clear,Some(closing.deadline)).await;
                            flush(&control, window.take(), &transport, &clear, Some(closing.deadline)).await;
                            let _ = closing.reply.send(());
                        }
                        return;
                    }
                    _ = deliver(&control,&mut pending,&transport,&clear,None) => {}
                }
            }
            envelope = records.recv() => match envelope {
                Some(envelope) => add(&control, &mut window, envelope),
                None => return,
            },
        }
    }
}

fn enabled(control: &Control) -> bool {
    control.metrics() && control.permitted()
}

fn drain(
    control: &Control,
    window: &mut Option<aggregate::Window>,
    records: &mut mpsc::Receiver<Envelope>,
) {
    for _ in 0..QUEUE_LIMIT {
        let Ok(envelope) = records.try_recv() else {
            return;
        };
        add(control, window, envelope);
    }
}

fn add(control: &Control, window: &mut Option<aggregate::Window>, envelope: Envelope) {
    let epoch = control.epoch();
    if !enabled(control) || envelope.epoch != epoch {
        return;
    }
    if window
        .as_ref()
        .is_some_and(|window| window.epoch() != epoch)
    {
        *window = None;
    }
    window
        .get_or_insert_with(|| aggregate::Window::new(epoch, control.metadata(), Utc::now()))
        .add(envelope.observation);
}

async fn flush(
    control: &Control,
    window: Option<aggregate::Window>,
    transport: &transport::Transport,
    clear: &Notify,
    deadline: Option<Instant>,
) {
    deliver(
        control,
        &mut window.map(Pending::from),
        transport,
        clear,
        deadline,
    )
    .await;
}

async fn deliver(
    control: &Control,
    pending: &mut Option<Pending>,
    transport: &transport::Transport,
    clear: &Notify,
    deadline: Option<Instant>,
) {
    let Some(pending) = pending else {
        return;
    };
    let epoch = pending.epoch;
    if !enabled(control) || control.epoch() != epoch {
        return;
    }
    // Remove an attempted batch before awaiting. Shutdown retains unsent batches
    // without blindly replaying a request whose receipt may be uncertain.
    while let Some(batch) = pending.batches.pop_front() {
        if !enabled(control) || control.epoch() != epoch {
            return;
        }
        tokio::select! {
            biased;
            _ = control.changed(epoch) => return,
            _ = clear.notified() => return,
            _ = transport.send(&batch, deadline) => {}
        }
    }
}

#[cfg(test)]
mod tests;
