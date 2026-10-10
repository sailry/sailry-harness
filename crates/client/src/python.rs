//! Transient dependency preparations share one Node-scoped observer.
use crate::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Topic, Update, python::Preparation};
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub preparations: Vec<Preparation>,
    pub connected: bool,
    pub error: Option<Fault>,
}

impl View {
    pub fn package(&self, name: &str) -> Option<&Preparation> {
        self.connected
            .then(|| self.preparations.iter().find(|item| item.package == name))
            .flatten()
    }
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.connected = false;
            view.preparations.clear();
        });
    }
}

impl Client {
    /// Reconnect reads active preparation only; it never restarts installation.
    pub async fn watch_python(
        &self,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let mut delay = Duration::from_millis(250);
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                result = self.transport.subscribe(Topic::Python) => result,
            };
            let error = match stream {
                Err(error) => error,
                Ok(mut stream) => loop {
                    let next = tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Ok(()),
                        _ = updates.closed() => return Ok(()),
                        next = stream.next() => next,
                    };
                    match next {
                        Ok(Update::Python { node, preparations }) if node == self.target() => {
                            delay = Duration::from_millis(250);
                            updates.send_replace(View {
                                preparations,
                                connected: true,
                                error: None,
                            });
                        }
                        Ok(_) => {
                            break Fault::new(
                                ErrorCode::WrongTarget,
                                "Python preparation belongs to another Node",
                            );
                        }
                        Err(error) => break error,
                    }
                },
            };
            updates.send_replace(View {
                error: Some(error.clone()),
                ..View::default()
            });
            if !matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                return Err(error);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                _ = tokio::time::sleep(delay) => {},
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests;
