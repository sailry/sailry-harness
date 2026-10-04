use super::*;
use crate::store::Ingress;
use rmcp::transport::auth::{AuthClient, AuthError};
use sailry_protocol::CredentialId;
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
    pub id: CredentialId,
}

type Client = AuthClient<reqwest::Client>;

/// Active transports for one grant share the SDK manager's token refresh lock.
#[derive(Default)]
pub(crate) struct Clients {
    active: Mutex<BTreeMap<Target, Arc<OnceCell<Client>>>>,
}

impl Clients {
    pub(crate) async fn client(
        &self,
        ingress: &Arc<Ingress>,
        target: Target,
    ) -> Result<Client, Fault> {
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
                && !active.contains_key(&target)
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "MCP authorization connection limit exceeded",
                ));
            }
            active.entry(target.clone()).or_default().clone()
        };
        let client = entry
            .get_or_try_init(|| async {
                let store = Store {
                    ingress: Arc::downgrade(ingress),
                    target: target.clone(),
                    revision: AtomicU64::new(0),
                };
                let mut manager = manager(&target.endpoint, store).await?;
                if !manager.initialize_from_store().await.map_err(unavailable)? {
                    return Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "MCP authorization is unavailable",
                    ));
                }
                let http = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
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
}

#[async_trait::async_trait]
impl CredentialStore for Store {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        let Some((revision, secret)) = ingress
            .mcp_credentials(self.target.clone())
            .await
            .map_err(|_| storage())?
        else {
            return Ok(None);
        };
        let credentials = serde_json::from_str(secret.expose()).map_err(|_| storage())?;
        self.revision.store(revision, Ordering::SeqCst);
        Ok(Some(credentials))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let ingress = self.ingress.upgrade().ok_or_else(storage)?;
        let encoded = serde_json::to_string(&credentials)
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
    AuthError::InternalError("MCP authorization is unavailable or changed".into())
}
