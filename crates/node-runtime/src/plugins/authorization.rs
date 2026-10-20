//! MCP OAuth uses the SDK's discovery, registration, PKCE and token lifecycle.
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationRequest, AuthorizationSession, CredentialStore,
    InMemoryCredentialStore, StoredCredentials,
};
use sailry_protocol::{ErrorCode, Fault, Secret};
use std::time::Duration;
mod clients;
mod credentials;
pub(crate) use clients::{Clients, Target};
use credentials::Credentials;
pub(crate) use credentials::Registered;

pub(crate) struct Flow {
    session: AuthorizationSession,
    credentials: InMemoryCredentialStore,
    client_secret: Option<Secret>,
}

impl Flow {
    pub(crate) async fn begin(
        endpoint: &str,
        redirect: &str,
        client_id: Option<&str>,
        registered: Option<Registered>,
        previous: Option<&Secret>,
    ) -> Result<Self, Fault> {
        let credentials = InMemoryCredentialStore::new();
        let mut manager = manager(endpoint, credentials.clone()).await?;
        let mut request = AuthorizationRequest::new(redirect).with_client_name("Sailry");
        let client_secret = if let Some(registered) = registered {
            if client_id.is_some_and(|id| id != registered.id) {
                return Err(Fault::invalid(
                    "MCP OAuth client ID differs from its configured secret",
                ));
            }
            request = request
                .with_preregistered_client(registered.id)
                .with_client_secret(registered.secret.expose());
            Some(registered.secret)
        } else if let Some(previous) = previous {
            let previous: Credentials =
                serde_json::from_str(previous.expose()).map_err(unavailable)?;
            if client_id.is_some_and(|id| id != previous.state.client_id) {
                return Err(Fault::invalid(
                    "MCP OAuth client ID differs from its saved registration",
                ));
            }
            request = request.with_preregistered_client(previous.state.client_id);
            if let Some(secret) = &previous.client_secret {
                request = request.with_client_secret(secret.expose());
            }
            previous.client_secret
        } else if let Some(client_id) = client_id {
            request = request.with_preregistered_client(client_id);
            None
        } else {
            // Capture the SDK registration result before its StoredCredentials drops the secret.
            let scopes = manager.select_scopes(None, &[]);
            let references = scopes.iter().map(String::as_str).collect::<Vec<_>>();
            let registered = manager
                .register_client("Sailry", redirect, &references)
                .await
                .map_err(unavailable)?;
            request = request
                .with_preregistered_client(registered.client_id)
                .with_scopes(scopes);
            if let Some(application_type) = registered.application_type {
                request = request.with_application_type(application_type);
            }
            let secret = registered.client_secret.map(Secret::new);
            if let Some(secret) = &secret {
                request = request.with_client_secret(secret.expose());
            }
            secret
        };
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
            client_secret,
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
        encode(&self.credentials, self.client_secret.clone()).await
    }
}

async fn manager(
    endpoint: &str,
    credentials: impl CredentialStore + 'static,
) -> Result<AuthorizationManager, Fault> {
    discovered_manager(endpoint, credentials)
        .await
        .map(|(manager, _, _)| manager)
}

async fn discovered_manager(
    endpoint: &str,
    credentials: impl CredentialStore + 'static,
) -> Result<(AuthorizationManager, bool, bool), Fault> {
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
    let resolution = manager.resolve_metadata().await.map_err(unavailable)?;
    let discovered = resolution.source.is_discovered();
    let metadata = resolution.metadata;
    let dynamic = metadata.registration_endpoint.is_some();
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
    Ok((manager, discovered, dynamic))
}

async fn encode(
    credentials: &InMemoryCredentialStore,
    client_secret: Option<Secret>,
) -> Result<Secret, Fault> {
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
    serde_json::to_string(&Credentials {
        state: stored,
        client_secret,
    })
    .map(Secret::new)
    .map_err(unavailable)
}

fn unavailable(_: impl std::fmt::Display) -> Fault {
    // SDK errors can contain provider responses; keep token and callback data private.
    Fault::new(ErrorCode::Unavailable, "MCP authorization failed")
}

fn invalid_callback() -> Fault {
    Fault::invalid("MCP authorization callback does not match its redirect")
}

/// Extract the SDK's current access token only inside the Node credential boundary.
pub(crate) fn access(secret: &Secret) -> Result<Secret, Fault> {
    let credentials: Credentials = serde_json::from_str(secret.expose()).map_err(unavailable)?;
    credentials
        .state
        .token_response
        .as_ref()
        .and_then(|response| serde_json::to_value(response).ok())
        .and_then(|value| {
            value
                .get("access_token")
                .and_then(serde_json::Value::as_str)
                .map(|value| Secret::new(value.to_owned()))
        })
        .ok_or_else(|| unavailable("MCP access token is unavailable"))
}
