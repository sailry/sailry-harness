//! Shared transient provider authorization observation.
pub(crate) mod observer;
use crate::Client;
use sailry_link::{CancellationToken, Subscription};
use sailry_protocol::{conversation::login, *};
use tokio::sync::watch;

pub type View = observer::View<login::Update>;
pub type Projection = observer::Projection<login::Update>;

impl observer::Progress for login::Update {
    type Attempt = login::Attempt;
    fn decode(update: Update) -> Result<Self, Fault> {
        let Update::ProviderLogin(update) = update else {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "provider authorization update expected",
            ));
        };
        Ok(update)
    }
    fn attempt(&self) -> &Self::Attempt {
        &self.attempt
    }
    fn revision(&self) -> u64 {
        self.revision
    }
    fn active(&self) -> bool {
        self.state.active()
    }
}

impl Client {
    pub async fn subscribe_login(&self, id: RequestId) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(Topic::ProviderLogin(id)).await
    }
    pub async fn watch_login(
        &self,
        attempt: login::Attempt,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        observer::watch(
            self,
            Topic::ProviderLogin(attempt.id),
            attempt,
            updates,
            stop,
        )
        .await
    }
}

#[cfg(test)]
mod tests;
