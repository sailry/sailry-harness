use super::*;
use crate::store::Ingress;
use rmcp::transport::auth::OAuthClientConfig;
use rmcp::transport::auth::{AuthClient, AuthError};
use sailry_protocol::plugin::connector::Id;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::{Mutex, OnceCell};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Target {
    pub plugin: String,
    pub server: String,
    pub endpoint: String,
    pub id: Id,
    pub revision: u64,
    pub checking: bool,
}

type Client = AuthClient<reqwest::Client>;
type Slot = Arc<OnceCell<Client>>;
type Registration = (String, Option<Secret>);
/// Active transports for one grant share the SDK manager's token refresh lock.
#[derive(Default)]
pub(crate) struct Clients {
    active: Mutex<BTreeMap<(Target, Option<String>), Slot>>,
}

impl Clients {
    pub(crate) async fn client(
        &self,
        ingress: &Arc<Ingress>,
        target: Target,
        registered: Option<Registered>,
    ) -> Result<Client, Fault> {
        let key = (
            target.clone(),
            registered.as_ref().map(Registered::fingerprint),
        );
        let entry = {
            let mut active = self.active.lock().await;
            active.retain(|_, cell| {
                Arc::strong_count(cell) > 1
                    || cell
                        .get()
                        .is_some_and(|client| Arc::strong_count(&client.auth_manager) > 1)
            });
            if active.len()
                >= sailry_protocol::plugin::MAX_INSTALLED * crate::plugins::mcp::MAX_SERVERS
                && !active.contains_key(&key)
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "MCP authorization connection limit exceeded",
                ));
            }
            active.entry(key).or_default().clone()
        };
        let client = entry
            .get_or_try_init(|| async {
                let client = Arc::new(Mutex::new(None));
                let store = Store {
                    ingress: Arc::downgrade(ingress),
                    target: target.clone(),
                    revision: AtomicU64::new(0),
                    client: client.clone(),
                    registered: registered.clone(),
                };
                let endpoint = ingress.plugins.connectors.resource(&target.endpoint);
                let mut manager = manager(&endpoint, store).await?;
                if !manager.initialize_from_store().await.map_err(unavailable)? {
                    return Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "MCP authorization is unavailable",
                    ));
                }
                let mut saved = client.lock().await;
                if let Some((id, secret)) = saved.as_mut() {
                    if let Some(registered) = &registered {
                        if *id != registered.id {
                            return Err(Fault::new(
                                ErrorCode::NotConfigured,
                                "MCP OAuth client changed; connect again",
                            ));
                        }
                        *secret = Some(registered.secret.clone());
                    }
                    if let Some(secret) = secret {
                        manager
                            .configure_client(
                                OAuthClientConfig::new(id.clone(), &endpoint)
                                    .with_client_secret(secret.expose()),
                            )
                            .map_err(unavailable)?;
                    }
                }
                drop(saved);
                let http = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .connect_timeout(Duration::from_secs(10))
                    .build()
                    .map_err(unavailable)?;
                Ok(AuthClient::new(http, manager))
            })
            .await?;
        Ok(client.clone())
    }
}

struct Store {
    ingress: Weak<Ingress>,
    target: Target,
    revision: AtomicU64,
    client: Arc<Mutex<Option<Registration>>>,
    registered: Option<Registered>,
}

#[async_trait::async_trait]
impl CredentialStore for Store {
    async fn acquire_refresh_guard(
        &self,
    ) -> Result<Option<rmcp::transport::auth::CredentialRefreshGuard>, AuthError> {
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        let lock = ingress
            .plugins
            .connectors
            .lock(self.target.id)
            .map_err(|_| storage())?;
        Ok(Some(rmcp::transport::auth::CredentialRefreshGuard::new(
            lock.lock_owned().await,
        )))
    }
    async fn on_refresh_token_rejected(
        &self,
        _credentials: &StoredCredentials,
    ) -> Result<(), AuthError> {
        self.clear().await
    }

    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        let Some((revision, secret)) = ingress
            .mcp_credentials(self.target.clone())
            .await
            .map_err(|_| storage())?
        else {
            return Ok(None);
        };
        let credentials: Credentials =
            serde_json::from_str(secret.expose()).map_err(|_| storage())?;
        let client_secret = self
            .registered
            .as_ref()
            .filter(|registered| registered.id == credentials.state.client_id)
            .map(|registered| registered.secret.clone())
            .or(credentials.client_secret);
        *self.client.lock().await = Some((credentials.state.client_id.clone(), client_secret));
        self.revision.store(revision, Ordering::SeqCst);
        Ok(Some(credentials.state))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        let saved = self.client.lock().await;
        let client_secret = saved
            .as_ref()
            .filter(|(id, _)| id == &credentials.client_id)
            .and_then(|(_, secret)| secret.clone());
        let encoded = serde_json::to_string(&Credentials {
            state: credentials,
            client_secret,
        })
        .map(Secret::new)
        .map_err(|_| storage())?;
        let revision = self.revision.load(Ordering::SeqCst);
        ingress
            .save_mcp_credentials(self.target.clone(), revision, Some(encoded))
            .await
            .map_err(|_| storage())?;
        self.revision.store(revision + 1, Ordering::SeqCst);
        Ok(())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        *self.client.lock().await = None;
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        ingress
            .save_mcp_credentials(
                self.target.clone(),
                self.revision.load(Ordering::SeqCst),
                None,
            )
            .await
            .map_err(|_| storage())
    }
}

fn storage() -> AuthError {
    AuthError::CredentialStoreError("MCP authorization is unavailable or changed".into())
}
