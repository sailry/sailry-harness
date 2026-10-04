//! MCP OAuth uses the SDK's discovery, registration, PKCE and token lifecycle.
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationRequest, AuthorizationSession, CredentialStore,
    InMemoryCredentialStore, StoredCredentials,
};
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::time::Duration;
mod clients;
pub(crate) use clients::{Clients, Target};

pub(crate) struct Flow {
    session: AuthorizationSession,
    credentials: InMemoryCredentialStore,
}

impl Flow {
    pub(crate) async fn begin(
        endpoint: &str,
        redirect: &str,
        client_id: Option<&str>,
    ) -> Result<Self, Fault> {
        let credentials = InMemoryCredentialStore::new();
        let manager = manager(endpoint, credentials.clone()).await?;
        let mut request = AuthorizationRequest::new(redirect).with_client_name("Sailry");
        if let Some(client_id) = client_id {
            request = request.with_preregistered_client(client_id);
        }
        let session = AuthorizationSession::new(manager, request)
            .await
            .map_err(|(_, error)| unavailable(error))?;
        if session.get_authorization_url().len() > 16 * 1024 {
            return Err(unavailable(
                "authorization URL exceeds the control frame limit",
            ));
        }
        crate::plugins::mcp::validate_http(session.get_authorization_url(), &Default::default())?;
        Ok(Self {
            session,
            credentials,
        })
    }

    pub(crate) fn url(&self) -> &str {
        self.session.get_authorization_url()
    }

    pub(crate) async fn complete(&self, callback: &Secret) -> Result<Secret, Fault> {
        let redirect = reqwest::Url::parse(&self.session.redirect_uri).map_err(unavailable)?;
        let mut returned = reqwest::Url::parse(callback.expose()).map_err(unavailable)?;
        if returned.fragment().is_some() {
            return Err(invalid_callback());
        }
        returned.set_query(None);
        if returned != redirect {
            return Err(invalid_callback());
        }
        self.session
            .handle_callback_url(callback.expose())
            .await
            .map_err(unavailable)?;
        encode(&self.credentials).await
    }
}

async fn manager(
    endpoint: &str,
    credentials: impl CredentialStore + 'static,
) -> Result<AuthorizationManager, Fault> {
    let mut manager = AuthorizationManager::new(endpoint)
        .await
        .map_err(unavailable)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(unavailable)?;
    manager.with_client(client).map_err(unavailable)?;
    manager.set_credential_store(credentials);
    let metadata = manager
        .resolve_metadata()
        .await
        .map_err(unavailable)?
        .metadata;
    for endpoint in [
        Some(&metadata.authorization_endpoint),
        Some(&metadata.token_endpoint),
        metadata.registration_endpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        crate::plugins::mcp::validate_http(endpoint, &Default::default())?;
    }
    manager.set_metadata(metadata);
    Ok(manager)
}

async fn encode(credentials: &InMemoryCredentialStore) -> Result<Secret, Fault> {
    let stored = credentials
        .load()
        .await
        .map_err(unavailable)?
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotConfigured,
                "MCP authorization has no credentials",
            )
        })?;
    serde_json::to_string(&stored)
        .map(Secret::new)
        .map_err(unavailable)
}

fn unavailable(_: impl std::fmt::Display) -> Fault {
    // SDK errors can contain provider responses; keep token and callback data private.
    Fault::new(ErrorCode::Unavailable, "MCP authorization failed")
}

fn invalid_callback() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "MCP authorization callback does not match its redirect",
    )
}
