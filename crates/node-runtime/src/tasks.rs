use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use crate::Error;

type Work = Pin<Box<dyn Future<Output = ()> + Send>>;

struct Task {
    work: Work,
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(crate) struct Tasks {
    sender: mpsc::Sender<Task>,
    capacity: Arc<Semaphore>,
}

pub(crate) struct Supervisor {
    pub(crate) tasks: Tasks,
    stop: Option<oneshot::Sender<Duration>>,
    worker: JoinHandle<Result<(), Error>>,
}

impl Tasks {
    /// Observer cancellation does not cancel an admitted task. This is memory admission,
    /// not a durable business receipt; reliable commands must first enter the Node store.
    pub(crate) fn spawn<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Result<oneshot::Receiver<T>, Error> {
        let permit = self.capacity.clone().try_acquire_owned().map_err(|_| {
            if self.capacity.is_closed() {
                Error::Stopped
            } else {
                Error::Busy
            }
        })?;
        let (reply, result) = oneshot::channel();
        self.sender
            .try_send(Task {
                work: Box::pin(async move {
                    let _ = reply.send(work.await);
                }),
                _permit: permit,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => Error::Busy,
                mpsc::error::TrySendError::Closed(_) => Error::Stopped,
            })?;
        Ok(result)
    }
}

impl Supervisor {
    pub(crate) fn start(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let capacity = Arc::new(Semaphore::new(capacity));
        let (stop, stopping) = oneshot::channel();
        Self {
            tasks: Tasks {
                sender,
                capacity: capacity.clone(),
            },
            stop: Some(stop),
            worker: tokio::spawn(serve(receiver, stopping, capacity)),
        }
    }

    pub(crate) async fn shutdown(mut self, grace: Duration) -> Result<(), Error> {
        self.tasks.capacity.close();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(grace);
        }
        (&mut self.worker)
            .await
            .map_err(|error| Error::Worker(error.to_string()))?
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.tasks.capacity.close();
        // Dropping the stop sender requests cancellation, never detaches active work.
        self.stop.take();
    }
}

async fn serve(
    mut receiver: mpsc::Receiver<Task>,
    mut stopping: oneshot::Receiver<Duration>,
    capacity: Arc<Semaphore>,
) -> Result<(), Error> {
    let mut active = JoinSet::new();
    let mut failure = None;
    let grace = loop {
        tokio::select! {
            biased;
            grace = &mut stopping => break grace.unwrap_or(Duration::ZERO),
            result = active.join_next(), if !active.is_empty() => {
                if let Some(Err(error)) = result { failure = Some(Error::Worker(error.to_string())); }
            }
            Some(task) = receiver.recv() => { active.spawn(run(task)); }
        }
    };
    capacity.close();
    receiver.close();
    let drain = async {
        while let Some(task) = receiver.recv().await {
            active.spawn(run(task));
        }
        while let Some(result) = active.join_next().await {
            if let Err(error) = result {
                failure = Some(Error::Worker(error.to_string()));
            }
        }
    };
    if tokio::time::timeout(grace, drain).await.is_err() {
        active.shutdown().await;
        return Err(Error::ShutdownTimeout);
    }
    failure.map_or(Ok(()), Err)
}

async fn run(task: Task) {
    let Task { work, _permit } = task;
    work.await;
}

#[cfg(test)]
mod tests;
