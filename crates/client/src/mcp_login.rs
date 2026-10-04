//! Shared transient MCP authorization observation.
use crate::Client;
use crate::login::observer;
use sailry_link::{CancellationToken, Subscription};
use sailry_protocol::{plugin::authorization as login, *};
use tokio::sync::watch;

pub type View = observer::View<login::Update>;
pub type Projection = observer::Projection<login::Update>;

impl observer::Progress for login::Update {
    type Attempt = login::Attempt;
    fn decode(update: Update) -> Result<Self, Fault> {
        let Update::McpLogin(update) = update else {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "MCP authorization update expected",
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
    pub async fn subscribe_mcp_login(&self, id: RequestId) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(Topic::McpLogin(id)).await
    }
    pub async fn watch_mcp_login(
        &self,
        attempt: login::Attempt,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        observer::watch(self, Topic::McpLogin(attempt.id), attempt, updates, stop).await
    }
}
