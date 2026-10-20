//! Connection observation never initiates or replays authorization.
use crate::{Client, login::observer};
use sailry_link::CancellationToken;
use sailry_protocol::{plugin::connector, *};
use tokio::sync::watch;

pub type View = observer::View<connector::LoginUpdate>;

impl observer::Progress for connector::LoginUpdate {
    type Attempt = connector::Attempt;
    fn decode(update: Update) -> Result<Self, Fault> {
        let Update::ConnectorLogin(update) = update else {
            return Err(Fault::invalid("connector authorization update expected"));
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
    pub async fn subscribe_connections(&self) -> Result<Box<dyn sailry_link::Subscription>, Fault> {
        self.transport.subscribe(Topic::Connections).await
    }
    pub async fn subscribe_connector_login(
        &self,
        id: RequestId,
    ) -> Result<Box<dyn sailry_link::Subscription>, Fault> {
        self.transport.subscribe(Topic::ConnectorLogin(id)).await
    }
    pub async fn watch_connector_login(
        &self,
        attempt: connector::Attempt,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        observer::watch(
            self,
            Topic::ConnectorLogin(attempt.id),
            attempt,
            updates,
            stop,
        )
        .await
    }
}
