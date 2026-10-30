use crate::telemetry::Control;
use sentry::{Envelope, Transport, protocol::EnvelopeItem, types::Dsn};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc as blocking,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

const CAPACITY: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Network {
    control: Arc<Control>,
    sender: mpsc::Sender<Command>,
    closed: AtomicBool,
    stop: tokio_util::sync::CancellationToken,
    #[cfg(test)]
    receipts: Arc<Receipts>,
}

#[cfg(test)]
#[derive(Default)]
struct Receipts {
    accepted: std::sync::atomic::AtomicUsize,
    failed: AtomicBool,
    changed: tokio::sync::Notify,
}

enum Command {
    Send(Box<Work>),
    Flush(blocking::SyncSender<()>),
    Close(blocking::SyncSender<()>),
}

struct Work {
    envelope: Envelope,
    epoch: u64,
    explicit: bool,
    cancel: tokio_util::sync::CancellationToken,
    complete: Option<oneshot::Sender<Result<(), ()>>>,
}

impl Network {
    pub(super) fn new(control: Arc<Control>, dsn: &Dsn) -> Option<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent("Sailry/1")
            .build()
            .ok()?;
        let endpoint = dsn.envelope_api_url();
        let authorization = dsn.to_auth(Some("sailry/1")).to_string();
        let (sender, mut receiver) = mpsc::channel(CAPACITY);
        let allowed = control.clone();
        let stop = tokio_util::sync::CancellationToken::new();
        let stopped = stop.clone();
        #[cfg(test)]
        let receipts = Arc::new(Receipts::default());
        #[cfg(test)]
        let observed = receipts.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        std::thread::Builder::new()
            .name("sailry-reports".into())
            .spawn(move || {
                runtime.block_on(async move {
                    loop {
                        let command = tokio::select! {
                            biased;
                            _ = stopped.cancelled() => None,
                            command = receiver.recv() => command,
                        };
                        let Some(command) = command else { break };
                        match command {
                            Command::Send(work) => {
                                let result = deliver(
                                    &allowed,
                                    &client,
                                    endpoint.as_str(),
                                    &authorization,
                                    &work,
                                )
                                .await;
                                #[cfg(test)]
                                {
                                    if result.is_ok() {
                                        observed.accepted.fetch_add(1, Ordering::AcqRel);
                                    } else {
                                        observed.failed.store(true, Ordering::Release);
                                    }
                                    observed.changed.notify_waiters();
                                }
                                if let Some(complete) = work.complete {
                                    let _ = complete.send(result);
                                }
                            }
                            Command::Flush(complete) => {
                                let _ = complete.send(());
                            }
                            Command::Close(complete) => {
                                let _ = complete.send(());
                                break;
                            }
                        }
                    }
                });
            })
            .ok()?;
        Some(Self {
            control,
            sender,
            closed: AtomicBool::new(false),
            stop,
            #[cfg(test)]
            receipts,
        })
    }

    #[cfg(test)]
    pub(super) async fn accepted(&self, count: usize) -> bool {
        let timeout = REQUEST_TIMEOUT.saturating_mul(count as u32) + Duration::from_secs(2);
        tokio::time::timeout(timeout, async {
            loop {
                let changed = self.receipts.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.receipts.failed.load(Ordering::Acquire) {
                    return false;
                }
                if self.receipts.accepted.load(Ordering::Acquire) >= count {
                    return true;
                }
                changed.await;
            }
        })
        .await
        .unwrap_or(false)
    }

    pub(super) async fn explicit(&self, envelope: Envelope, epoch: u64) -> Result<(), ()> {
        if self.closed.load(Ordering::Acquire)
            || !self.control.permitted()
            || self.control.epoch() != epoch
        {
            return Err(());
        }
        let (complete, receiver) = oneshot::channel();
        let cancel = self.stop.child_token();
        let _cancel = cancel.clone().drop_guard();
        self.sender
            .try_send(Command::Send(Box::new(Work {
                envelope,
                epoch,
                explicit: true,
                cancel,
                complete: Some(complete),
            })))
            .map_err(|_| ())?;
        tokio::select! {
            biased;
            _ = self.control.changed(epoch) => Err(()),
            result = receiver => result.map_err(|_| ())?,
            _ = tokio::time::sleep(REQUEST_TIMEOUT + Duration::from_secs(5)) => Err(()),
        }
    }

    fn barrier(&self, timeout: Duration, close: bool) -> bool {
        if close && self.closed.swap(true, Ordering::AcqRel) {
            return true;
        }
        let (complete, receiver) = blocking::sync_channel(1);
        let command = if close {
            Command::Close(complete)
        } else {
            Command::Flush(complete)
        };
        if self.sender.try_send(command).is_err() {
            if close {
                self.stop.cancel();
            }
            return false;
        }
        let completed = receiver.recv_timeout(timeout).is_ok();
        if close && !completed {
            self.stop.cancel();
        }
        completed
    }
}

impl Transport for Network {
    fn send_envelope(&self, envelope: Envelope) {
        if self.closed.load(Ordering::Acquire) || !self.control.errors() {
            return;
        }
        // Scope attachments and non-error SDK items are never automatic reports.
        // Only the explicit diagnostics path may attach a sanitized log tail.
        let mut safe = Envelope::new();
        let mut epoch = None;
        for item in envelope.into_items() {
            if let EnvelopeItem::Event(mut event) = item {
                epoch = event
                    .extra
                    .remove("_telemetry_epoch")
                    .and_then(|value| value.as_u64());
                safe.add_item(*event);
            }
        }
        let Some(epoch) = epoch else { return };
        if safe.event().is_none() || epoch != self.control.epoch() || !self.control.errors() {
            return;
        }
        let _ = self.sender.try_send(Command::Send(Box::new(Work {
            envelope: safe,
            epoch,
            explicit: false,
            cancel: self.stop.clone(),
            complete: None,
        })));
    }

    fn flush(&self, timeout: Duration) -> bool {
        if timeout.is_zero() {
            return false;
        }
        self.barrier(timeout, false)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        self.barrier(timeout, true)
    }
}

async fn deliver(
    control: &Control,
    client: &reqwest::Client,
    endpoint: &str,
    authorization: &str,
    work: &Work,
) -> Result<(), ()> {
    let permitted = || {
        control.permitted()
            && control.epoch() == work.epoch
            && (work.explicit || control.errors())
            && !work
                .complete
                .as_ref()
                .is_some_and(oneshot::Sender::is_closed)
    };
    if !permitted() {
        return Err(());
    }
    let mut body = Vec::new();
    work.envelope.to_writer(&mut body).map_err(|_| ())?;
    if !permitted() {
        return Err(());
    }
    // No request URL, response body or HTTP error text reaches diagnostics.
    tokio::select! {
        biased;
        _ = work.cancel.cancelled() => Err(()),
        _ = control.changed(work.epoch) => Err(()),
        result = client.post(endpoint)
            .header("x-sentry-auth", authorization)
            .header("content-type", "application/x-sentry-envelope")
            .body(body)
            .send() => {
                result.map_err(|_| ())?.status().is_success().then_some(()).ok_or(())
            }
    }
}
