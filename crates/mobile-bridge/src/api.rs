pub(crate) mod activity;
pub(crate) mod commands;
pub(crate) mod connection;
pub(crate) mod conversation;
pub(crate) mod login;
pub(crate) mod overview;
pub(crate) mod ports;
pub(crate) mod speech;
pub(crate) mod terminal;
pub(crate) mod transfers;
pub(crate) mod usage;
pub use commands::CommandUpdates;
pub use connection::{Connection, Updates};
pub use conversation::ConversationUpdates;
pub use login::LoginUpdates;
pub use overview::OverviewUpdates;
pub use ports::Forwarding;
pub use speech::SpeechInput;
pub use terminal::TerminalUpdates;
pub use transfers::{Digest, Download, Upload};
pub use usage::UsageUpdates;

use flutter_rust_bridge::frb;
use sailry_link::{CancellationToken, Link, LinkHandle, NetworkScope};
use tokio::sync::Mutex;

#[frb(opaque)]
pub struct Controller {
    link: Mutex<Option<Link>>,
    handle: LinkHandle,
    stop: CancellationToken,
    inbox: std::sync::Arc<Mutex<sailry_client::activity::Inbox>>,
}

impl Controller {
    pub fn set_name(&self, name: String) -> Result<(), String> {
        self.check()?;
        self.handle.set_name(name).map_err(error)
    }

    pub async fn open(path: String, internet: bool, relays: Vec<String>) -> Result<Self, String> {
        #[cfg(target_os = "android")]
        crate::android::ready()?;
        let scope = if !relays.is_empty() {
            NetworkScope::CustomRelays(relays)
        } else if internet {
            NetworkScope::Internet
        } else {
            NetworkScope::default()
        };
        let link = Link::controller(path, scope).await.map_err(error)?;
        Ok(Self {
            handle: link.handle(),
            link: Mutex::new(Some(link)),
            stop: CancellationToken::new(),
            inbox: Default::default(),
        })
    }

    pub async fn pair(&self, ticket: String) -> Result<String, String> {
        self.check()?;
        let address = self.handle.pair(&ticket).await.map_err(error)?;
        serde_json::to_string(&address).map_err(error)
    }

    pub async fn pair_pin(&self, origin: String, pin: String) -> Result<String, String> {
        use sailry_link::rendezvous::{DEFAULT_SERVICE, Relay, RequestId};
        self.check()?;
        let origin = if origin.trim().is_empty() {
            DEFAULT_SERVICE
        } else {
            origin.trim()
        };
        let address = Relay::new(origin)
            .map_err(error)?
            .pair(&self.handle, &pin, &RequestId::default())
            .await
            .map_err(error)?;
        serde_json::to_string(&address).map_err(error)
    }

    pub async fn peers(&self) -> Result<Vec<String>, String> {
        self.check()?;
        self.handle
            .peers()
            .await
            .map_err(error)?
            .iter()
            .map(|address| serde_json::to_string(address).map_err(error))
            .collect()
    }

    /// Notify on foreground resume or a platform connectivity callback.
    pub async fn network_changed(&self) -> Result<(), String> {
        self.check()?;
        self.handle.network_changed().await.map_err(error)
    }

    pub fn connect(&self, address: String) -> Result<Connection, String> {
        self.check()?;
        let address = serde_json::from_str(&address).map_err(error)?;
        Ok(Connection::new(
            self.handle.remote(address),
            self.stop.child_token(),
            self.inbox.clone(),
        ))
    }

    pub async fn close(&self) -> Result<(), String> {
        self.stop.cancel();
        let mut link = self.link.lock().await;
        if let Some(link) = link.take() {
            link.close().await.map_err(error)?;
        }
        Ok(())
    }
}

impl Controller {
    fn check(&self) -> Result<(), String> {
        if self.stop.is_cancelled() {
            Err("controller is closed".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
