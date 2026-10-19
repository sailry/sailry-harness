//! A single blocking owner keeps QuickJS values on their originating thread.
use crate::plugins::script::{Bundle, translation::Session};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

struct Call {
    handler: &'static str,
    input: Value,
    reply: oneshot::Sender<Result<Value, Fault>>,
}

pub(super) struct Worker {
    sender: mpsc::Sender<Call>,
    stop: CancellationToken,
    pub(super) classifies: bool,
}

impl Worker {
    pub(super) async fn new(
        bundle: Bundle,
        stop: &CancellationToken,
        budget: usize,
    ) -> Result<Self, Fault> {
        let (sender, mut calls) = mpsc::channel::<Call>(1);
        let (ready, started) = oneshot::channel();
        let stop = stop.child_token();
        let interrupted = stop.clone();
        tokio::task::spawn_blocking(move || {
            let mut session = match Session::model(bundle, interrupted.clone()) {
                Ok(session) => {
                    match session.has_export("classify") {
                        Ok(classifies) => {
                            let _ = ready.send(Ok(classifies));
                        }
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    }
                    session
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            while let Some(call) = calls.blocking_recv() {
                if interrupted.is_cancelled() {
                    break;
                }
                let budget = if call.handler == "prepare" {
                    budget.min(super::transport::REQUEST_BYTES)
                } else {
                    budget
                };
                let result = session.call(call.handler, &call.input, budget);
                let failed = result.is_err();
                let _ = call.reply.send(result);
                if failed {
                    break;
                }
            }
        });
        tokio::select! {
            _ = stop.cancelled() => Err(cancelled()),
            result = started => { let classifies=result.map_err(|_|unavailable())??; Ok(Self { sender, stop, classifies }) }
        }
    }

    pub(super) async fn call<T: DeserializeOwned>(
        &self,
        handler: &'static str,
        input: &impl Serialize,
    ) -> Result<T, Fault> {
        let input = serde_json::to_value(input).map_err(|_| unavailable())?;
        let (reply, result) = oneshot::channel();
        tokio::select! {
            _ = self.stop.cancelled() => return Err(cancelled()),
            sent = self.sender.send(Call { handler, input, reply }) => sent.map_err(|_|unavailable())?,
        }
        let value = tokio::select! {
            _ = self.stop.cancelled() => return Err(cancelled()),
            result = result => result.map_err(|_|unavailable())??,
        };
        serde_json::from_value(value)
            .map_err(|_| Fault::invalid("model translation returned an invalid value"))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "model translation was cancelled")
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "model translation is unavailable")
}
