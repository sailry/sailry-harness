//! Shared account login observation does not initiate or replay authorization.
use crate::{Client, login::observer};
use sailry_link::CancellationToken;
use sailry_protocol::{plugin::oauth as login, *};
use tokio::sync::watch;

pub type View = observer::View<login::Update>;

impl observer::Progress for login::Update {
    type Attempt = login::Attempt;
    fn decode(update: Update) -> Result<Self, Fault> {
        let Update::PluginLogin(update) = update else {
            return Err(Fault::invalid("plugin authorization update expected"));
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
    pub async fn subscribe_plugin_login(
        &self,
        id: RequestId,
    ) -> Result<Box<dyn sailry_link::Subscription>, Fault> {
        self.transport.subscribe(Topic::PluginLogin(id)).await
    }
    pub async fn watch_plugin_login(
        &self,
        attempt: login::Attempt,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        observer::watch(self, Topic::PluginLogin(attempt.id), attempt, updates, stop).await
    }
}
