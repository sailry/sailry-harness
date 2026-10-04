//! Official Cua tools execute on the owning Node through one SDK runtime.
mod catalog;
mod driver;
pub(crate) mod execute;
pub(crate) mod permissions;
mod worker;
pub use worker::Worker;
#[cfg(test)]
mod tests;

pub(crate) use catalog::{catalog, definition, read_only};
use sailry_link::CancellationToken;
use sailry_protocol::SessionId;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
pub(crate) type Result<T> = std::result::Result<T, String>;

#[derive(Default)]
pub(crate) struct Desktop {
    state: Arc<Mutex<driver::Runtime>>,
    pub stop: CancellationToken,
}

impl Desktop {
    pub(crate) async fn configure_worker(&self, worker: Option<Worker>) {
        self.state.lock().await.worker = worker;
    }

    pub async fn execute(
        &self,
        session: SessionId,
        name: &str,
        arguments: Value,
        stop: &CancellationToken,
        ready: impl std::future::Future<Output = Result<()>>,
    ) -> Result<Value> {
        let mut state = tokio::select! {
            biased;
            _ = stop.cancelled() => return Err("computer operation cancelled".into()),
            _ = self.stop.cancelled() => return Err("execution Node is shutting down".into()),
            guard = self.state.lock() => guard,
        };
        ready.await?;
        // Cancellation cannot prove whether an admitted native action occurred.
        // Retain the SDK session, and never replay interrupted input.
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err("computer operation interrupted; effects may have occurred".into()),
            _ = self.stop.cancelled() => Err("execution Node stopped; effects may have occurred".into()),
            result = state.call(session, name, arguments) => result,
        }
    }

    pub async fn finish(&self, session: SessionId) {
        self.state.lock().await.finish(session).await;
    }

    pub async fn shutdown(&self) {
        self.stop.cancel();
        self.state.lock().await.shutdown().await;
    }
}

fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}
