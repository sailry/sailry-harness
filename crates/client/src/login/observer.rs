//! Shared transient login observation. Reconnection never initiates authorization.
use crate::{Apply, Client};
use sailry_link::CancellationToken;
use sailry_protocol::*;
use std::time::Duration;
use tokio::sync::watch;

pub trait Progress: Clone + PartialEq + serde::Serialize + Send + Sync + 'static {
    type Attempt: Clone + PartialEq + Send + Sync;
    fn decode(update: Update) -> Result<Self, Fault>;
    fn attempt(&self) -> &Self::Attempt;
    fn revision(&self) -> u64;
    fn active(&self) -> bool;
}

#[derive(Clone, serde::Serialize)]
pub struct View<P> {
    pub update: Option<P>,
    pub connected: bool,
    pub error: Option<Fault>,
}

pub struct Projection<P: Progress> {
    attempt: P::Attempt,
    generation: u64,
    update: Option<P>,
}

impl<P: Progress> Projection<P> {
    pub fn new(attempt: P::Attempt) -> Self {
        Self {
            attempt,
            generation: 0,
            update: None,
        }
    }
    pub fn update(&self) -> Option<&P> {
        self.update.as_ref()
    }
    pub fn reconnect(&mut self, generation: u64) -> Result<(), Fault> {
        if generation <= self.generation {
            return Err(invalid("connection generation must increase"));
        }
        self.generation = generation;
        Ok(())
    }
    pub fn apply(&mut self, generation: u64, update: Update) -> Result<Apply, Fault> {
        if generation != self.generation {
            return Ok(Apply::Ignored);
        }
        let update = P::decode(update)?;
        if update.attempt() != &self.attempt || update.revision() == 0 {
            return Err(invalid(
                "authorization update belongs to another attempt or has an invalid revision",
            ));
        }
        if let Some(current) = &self.update {
            if update.revision() < current.revision() {
                return Ok(Apply::Ignored);
            }
            if update.revision() == current.revision() {
                return if update == *current {
                    Ok(Apply::Ignored)
                } else {
                    Err(invalid("authorization revision changed its contents"))
                };
            }
            if !current.active() {
                return Err(invalid("completed authorization cannot change"));
            }
        }
        self.update = Some(update);
        Ok(Apply::Applied)
    }
}

impl<P> Default for View<P> {
    fn default() -> Self {
        Self {
            update: None,
            connected: false,
            error: None,
        }
    }
}

struct Closing<P>(watch::Sender<View<P>>);
impl<P> Drop for Closing<P> {
    fn drop(&mut self) {
        self.0.send_modify(|view| view.connected = false);
    }
}

pub(crate) async fn watch<P: Progress>(
    client: &Client,
    topic: Topic,
    attempt: P::Attempt,
    updates: watch::Sender<View<P>>,
    stop: CancellationToken,
) -> Result<(), Fault> {
    let _closing = Closing(updates.clone());
    let mut projection = Projection::<P>::new(attempt);
    let mut generation = 0;
    let mut delay = Duration::from_millis(250);
    loop {
        generation += 1;
        projection.reconnect(generation)?;
        let stream = tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            _ = updates.closed() => return Ok(()),
            result = client.transport.subscribe(topic.clone()) => result,
        };
        let failure = match stream {
            Ok(mut stream) => loop {
                let result = tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    _ = updates.closed() => return Ok(()),
                    result = stream.next() => result,
                };
                match result.and_then(|update| projection.apply(generation, update)) {
                    Ok(_) => {
                        updates.send_replace(View {
                            update: projection.update().cloned(),
                            connected: true,
                            error: None,
                        });
                        if projection.update().is_some_and(|update| !update.active()) {
                            return Ok(());
                        }
                        delay = Duration::from_millis(250);
                    }
                    Err(error) => break error,
                }
            },
            Err(error) => error,
        };
        updates.send_modify(|view| {
            view.connected = false;
            view.error = Some(failure.clone());
        });
        if !matches!(failure.code, ErrorCode::Unavailable | ErrorCode::Busy) {
            return Err(failure);
        }
        tokio::select! {
            _ = stop.cancelled() => return Ok(()),
            _ = updates.closed() => return Ok(()),
            _ = tokio::time::sleep(delay) => {},
        }
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

fn invalid(message: &'static str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
