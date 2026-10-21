//! Scoped MCP credentials share the existing SDK framing without replaying POSTs.
use super::*;
use crate::store::Ingress;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::{
        auth::AuthClient,
        common::client_side_sse::BoxedSseResponse,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use std::collections::HashMap;

type Headers = HashMap<HeaderName, HeaderValue>;
type Error = StreamableHttpError<reqwest::Error>;

#[derive(Clone)]
pub(crate) struct Authorization {
    pub ingress: Arc<Ingress>,
    pub targets: Vec<(Target, Injection)>,
    pub stop: CancellationToken,
    pub oauth: Option<AuthClient<reqwest::Client>>,
    pub settings: Option<Arc<(sailry_protocol::plugin::Context, String)>>,
    redactions: Arc<Mutex<Vec<Secret>>>,
    sealed: Arc<Mutex<Vec<(Id, Credential, Secret)>>>,
}
impl Authorization {
    pub(crate) fn new(
        ingress: Arc<Ingress>,
        targets: Vec<(Target, Injection)>,
        stop: CancellationToken,
    ) -> Self {
        let redactions = targets
            .iter()
            .flat_map(|(target, _)| target.secrets.values().cloned())
            .collect();
        Self {
            ingress,
            targets,
            stop,
            oauth: None,
            settings: None,
            redactions: Arc::new(Mutex::new(redactions)),
            sealed: Arc::default(),
        }
    }
    pub(crate) fn key(&self) -> Vec<(Id, u64)> {
        self.targets
            .iter()
            .map(|(target, _)| (target.id, target.revision))
            .collect()
    }
    /// Confirms every target and sealed credential is still current.
    pub(crate) async fn validate(&self) -> Result<(), Fault> {
        if let Some(settings) = &self.settings {
            self.ingress
                .mcp_connectors(settings.0.clone(), settings.1.clone())
                .await?;
        }
        for (target, _) in &self.targets {
            self.ingress
                .connector_target(target.id, target.revision, target.checking)
                .await?;
        }
        let sealed = self.sealed.lock().map_err(|_| unavailable())?.clone();
        for (id, credential, expected) in sealed {
            let target = &self
                .targets
                .iter()
                .find(|(target, injection)| {
                    target.id == id
                        && target
                            .credential(injection)
                            .is_ok_and(|selected| selected == credential)
                })
                .ok_or_else(unavailable)?
                .0;
            let current = self
                .ingress
                .plugins
                .connectors
                .access_for(target, &credential, &self.ingress, None, &self.stop)
                .await?;
            if current.is_none_or(|current| current.token != expected) {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "MCP credentials changed",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn seal(&self, id: Id, credential: Credential, token: Secret) -> Result<(), Fault> {
        self.sealed
            .lock()
            .map_err(|_| unavailable())?
            .push((id, credential, token));
        Ok(())
    }
    pub(crate) fn redact(&self, value: &mut serde_json::Value) {
        fn replace(value: &mut serde_json::Value, secrets: &[Secret]) {
            match value {
                serde_json::Value::String(text) => {
                    for secret in secrets {
                        if !secret.expose().is_empty() {
                            *text = text.replace(secret.expose(), "[redacted]");
                        }
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        replace(value, secrets);
                    }
                }
                serde_json::Value::Object(values) => {
                    for value in values.values_mut() {
                        replace(value, secrets);
                    }
                }
                _ => {}
            }
        }
        if let Ok(secrets) = self.redactions.lock() {
            replace(value, &secrets);
        }
    }
    pub(crate) fn client(&self, inner: reqwest::Client, max: usize) -> Client {
        Client {
            inner,
            authorization: self.clone(),
            oauth: self.oauth.clone(),
            max,
        }
    }
    pub(crate) fn remember(&self, token: Secret) -> Result<(), Fault> {
        let mut secrets = self.redactions.lock().map_err(|_| unavailable())?;
        if !secrets.contains(&token) {
            secrets.push(token);
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct Client {
    inner: reqwest::Client,
    authorization: Authorization,
    oauth: Option<AuthClient<reqwest::Client>>,
    max: usize,
}
struct Prepared {
    uri: Arc<str>,
    headers: Headers,
    token: Option<String>,
    revision: Option<u64>,
    sent: Vec<(Target, Credential, Access)>,
}
impl Client {
    async fn prepare(&self, uri: Arc<str>, mut headers: Headers) -> Result<Prepared, Error> {
        self.authorization.validate().await.map_err(failure)?;
        let mut sent = Vec::new();
        for (target, injection) in &self.authorization.targets {
            if !target.endpoint(&uri) {
                return Err(failure(unavailable()));
            }
            let credential = target.credential(injection).map_err(failure)?;
            if target.method == Kind::McpOauth && credential == Credential::Grant {
                continue;
            }
            let token = self
                .authorization
                .ingress
                .plugins
                .connectors
                .access_for(
                    target,
                    &credential,
                    &self.authorization.ingress,
                    None,
                    &self.authorization.stop,
                )
                .await
                .map_err(failure)?;
            let Some(token) = token else {
                continue;
            };
            let name = HeaderName::from_bytes(
                injection
                    .header
                    .as_deref()
                    .ok_or_else(|| failure(unavailable()))?
                    .as_bytes(),
            )
            .map_err(|_| failure(unavailable()))?;
            let mut value =
                HeaderValue::from_str(&format!("{}{}", injection.prefix, token.token.expose()))
                    .map_err(|_| failure(unavailable()))?;
            value.set_sensitive(true);
            if headers.get(&name).is_some_and(|value| !value.is_empty()) {
                return Err(failure(Fault::invalid(
                    "MCP credential header is already configured",
                )));
            }
            headers.insert(name, value);
            self.authorization
                .remember(token.token.clone())
                .map_err(failure)?;
            sent.push((target.clone(), credential, token));
        }
        let (token, revision) = if let Some(client) = &self.oauth {
            client
                .get_access_token()
                .await
                .map_err(|_| failure(unavailable()))?;
            let target = self.oauth_target().map_err(failure)?;
            let live = self
                .authorization
                .ingress
                .connector_target(target.id, target.revision, target.checking)
                .await
                .map_err(failure)?;
            let token = Credential::Grant
                .read(&live)
                .map_err(failure)?
                .ok_or_else(|| failure(unavailable()))?;
            self.authorization
                .remember(token.clone())
                .map_err(failure)?;
            (Some(token.expose().to_owned()), Some(live.grant_revision))
        } else {
            (None, None)
        };
        let uri = self
            .authorization
            .ingress
            .plugins
            .connectors
            .resource(&uri)
            .into();
        Ok(Prepared {
            uri,
            headers,
            token,
            revision,
            sent,
        })
    }
    fn oauth_target(&self) -> Result<&Target, Fault> {
        self.authorization
            .targets
            .iter()
            .find(|(target, _)| target.method == Kind::McpOauth)
            .map(|(target, _)| target)
            .ok_or_else(unavailable)
    }
    fn accepted(&self, prepared: &Prepared) -> Result<(), Fault> {
        let service = &self.authorization.ingress.plugins.connectors;
        for (target, _, access) in &prepared.sent {
            service.accepted(target.id, access.revision)?;
        }
        if let Some(revision) = prepared.revision {
            service.accepted(self.oauth_target()?.id, revision)?;
        }
        Ok(())
    }
    async fn rejected(&self, prepared: &Prepared, repeated: bool) -> Result<bool, Error> {
        let ingress = &self.authorization.ingress;
        let service = &ingress.plugins.connectors;
        let mut changed = false;
        if let (Some(client), Some(revision)) = (&self.oauth, prepared.revision) {
            let target = self.oauth_target().map_err(failure)?;
            match service
                .reject(target, ingress, revision, repeated)
                .await
                .map_err(failure)?
            {
                Rejection::Stop => {}
                rejection => {
                    if matches!(rejection, Rejection::Renew) {
                        client
                            .auth_manager
                            .lock()
                            .await
                            .refresh_token()
                            .await
                            .map_err(|_| failure(unavailable()))?;
                        let live = ingress
                            .connector_target(target.id, target.revision, target.checking)
                            .await
                            .map_err(failure)?;
                        service
                            .rotated(target.id, live.grant_revision)
                            .map_err(failure)?;
                    }
                    changed = true;
                }
            }
        }
        for (target, credential, sent) in &prepared.sent {
            let force = match service
                .reject(target, ingress, sent.revision, repeated)
                .await
                .map_err(failure)?
            {
                Rejection::Stop => continue,
                Rejection::Current => None,
                Rejection::Renew => Some(sent.revision),
            };
            let access = service
                .access_for(target, credential, ingress, force, &self.authorization.stop)
                .await
                .map_err(failure)?;
            if let Some(access) = access {
                self.authorization.remember(access.token).map_err(failure)?;
            }
            changed = true;
        }
        Ok(changed)
    }
}
fn failure(_: Fault) -> Error {
    Error::UnexpectedServerResponse("MCP authorization is unavailable or changed".into())
}
fn unauthorized<T>(result: &Result<T, Error>) -> bool {
    matches!(result, Err(Error::AuthRequired(_)))
        || matches!(result,Err(Error::HttpStatus(error)) if error.status==401)
}
/// Any other server status means the credential itself was not rejected.
fn answered<T>(result: &Result<T, Error>) -> bool {
    match result {
        Ok(_) => true,
        Err(Error::HttpStatus(error)) => error.status != 401,
        Err(Error::Client(error)) => error.status().is_some_and(|status| status.as_u16() != 401),
        Err(
            Error::InsufficientScope(_)
            | Error::ServerDoesNotSupportSse
            | Error::SessionExpired
            | Error::UnexpectedContentType(_),
        ) => true,
        _ => false,
    }
}

impl StreamableHttpClient for Client {
    type Error = reqwest::Error;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: Headers,
    ) -> Result<StreamableHttpPostResponse, Error> {
        self.post_message_with_max_sse_event_size(uri, message, session_id, auth, headers, self.max)
            .await
    }
    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth: Option<String>,
        headers: Headers,
        max: usize,
    ) -> Result<StreamableHttpPostResponse, Error> {
        if auth.is_some() {
            return Err(failure(unavailable()));
        }
        let prepared = self.prepare(uri, headers).await?;
        let result = self
            .inner
            .post_message_with_max_sse_event_size(
                prepared.uri.clone(),
                message,
                session_id,
                prepared.token.clone(),
                prepared.headers.clone(),
                max,
            )
            .await;
        // A rejected POST can rotate credentials, but its side effect is never replayed.
        if unauthorized(&result) {
            self.rejected(&prepared, false).await?;
        } else if answered(&result) {
            self.accepted(&prepared).map_err(failure)?;
        }
        result.map_err(|_| failure(unavailable()))
    }
    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth: Option<String>,
        headers: Headers,
    ) -> Result<(), Error> {
        if auth.is_some() {
            return Err(failure(unavailable()));
        }
        let prepared = self.prepare(uri, headers).await?;
        self.inner
            .delete_session(prepared.uri, session_id, prepared.token, prepared.headers)
            .await
            .map_err(|_| failure(unavailable()))
    }
    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth: Option<String>,
        headers: Headers,
    ) -> Result<BoxedSseResponse, Error> {
        self.get_stream_with_max_sse_event_size(
            uri,
            session_id,
            last_event_id,
            auth,
            headers,
            self.max,
        )
        .await
    }
    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth: Option<String>,
        headers: Headers,
        max: usize,
    ) -> Result<BoxedSseResponse, Error> {
        if auth.is_some() {
            return Err(failure(unavailable()));
        }
        let prepared = self.prepare(uri.clone(), headers.clone()).await?;
        let result = self
            .inner
            .get_stream_with_max_sse_event_size(
                prepared.uri.clone(),
                session_id.clone(),
                last_event_id.clone(),
                prepared.token.clone(),
                prepared.headers.clone(),
                max,
            )
            .await;
        if unauthorized(&result) && self.rejected(&prepared, false).await? {
            let fresh = self.prepare(uri, headers).await?;
            let result = self
                .inner
                .get_stream_with_max_sse_event_size(
                    fresh.uri.clone(),
                    session_id,
                    last_event_id,
                    fresh.token.clone(),
                    fresh.headers.clone(),
                    max,
                )
                .await;
            if unauthorized(&result) {
                self.rejected(&fresh, true).await?;
            } else if answered(&result) {
                self.accepted(&fresh).map_err(failure)?;
            }
            return result.map_err(|_| failure(unavailable()));
        }
        if answered(&result) {
            self.accepted(&prepared).map_err(failure)?;
        }
        result.map_err(|_| failure(unavailable()))
    }
}

#[cfg(test)]
mod tests;
