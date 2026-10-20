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
    sealed: Arc<Mutex<Vec<(Id, Secret)>>>,
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
    pub(crate) async fn validate(&self) -> Result<(), Fault> {
        if let Some(settings) = &self.settings {
            self.ingress
                .mcp_connectors(settings.0.clone(), settings.1.clone())
                .await?;
        }
        for (target, _) in &self.targets {
            self.ingress
                .connector_target(target.id, target.revision)
                .await?;
        }
        let sealed = self.sealed.lock().map_err(|_| unavailable())?.clone();
        for (id, expected) in sealed {
            let target = &self
                .targets
                .iter()
                .find(|(target, _)| target.id == id)
                .ok_or_else(unavailable)?
                .0;
            let current = self
                .ingress
                .plugins
                .connectors
                .access(target, &self.ingress, None, &self.stop)
                .await?;
            if current != expected {
                return Err(Fault::new(
                    ErrorCode::NotConfigured,
                    "MCP credentials changed",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn seal(&self, id: Id, token: Secret) -> Result<(), Fault> {
        self.sealed
            .lock()
            .map_err(|_| unavailable())?
            .push((id, token));
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
    sent: Vec<(Target, Secret)>,
}
impl Client {
    async fn prepare(&self, uri: Arc<str>, mut headers: Headers) -> Result<Prepared, Error> {
        self.authorization.validate().await.map_err(failure)?;
        let mut sent = Vec::new();
        for (target, injection) in &self.authorization.targets {
            if !target.endpoint(&uri) {
                return Err(failure(unavailable()));
            }
            if target.method == Kind::McpOauth {
                continue;
            }
            let token = if let Some(field) = &injection.field {
                target.secrets.get(field).cloned()
            } else if target.method == Kind::Secret
                && target.secrets.is_empty()
                && target.declaration.fields.iter().all(|field| field.optional)
            {
                None
            } else {
                Some(
                    self.authorization
                        .ingress
                        .plugins
                        .connectors
                        .access(
                            target,
                            &self.authorization.ingress,
                            None,
                            &self.authorization.stop,
                        )
                        .await
                        .map_err(failure)?,
                )
            };
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
                HeaderValue::from_str(&format!("{}{}", injection.prefix, token.expose()))
                    .map_err(|_| failure(unavailable()))?;
            value.set_sensitive(true);
            if headers.get(&name).is_some_and(|value| !value.is_empty()) {
                return Err(failure(Fault::invalid(
                    "MCP credential header is already configured",
                )));
            }
            headers.insert(name, value);
            self.authorization
                .remember(token.clone())
                .map_err(failure)?;
            sent.push((target.clone(), token));
        }
        let token = if let Some(client) = &self.oauth {
            let token = client
                .get_access_token()
                .await
                .map_err(|_| failure(unavailable()))?;
            self.authorization
                .remember(Secret::new(token.clone()))
                .map_err(failure)?;
            Some(token)
        } else {
            None
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
            sent,
        })
    }
    async fn rejected(&self, prepared: &Prepared) -> Result<bool, Error> {
        let mut changed = false;
        if let (Some(client), Some(sent)) = (&self.oauth, &prepared.token) {
            let manager = client.auth_manager.lock().await;
            let current = manager
                .get_access_token()
                .await
                .map_err(|_| failure(unavailable()))?;
            let current = if current != *sent {
                current
            } else {
                {
                    manager
                        .refresh_token()
                        .await
                        .map_err(|_| failure(unavailable()))?;
                    manager
                        .get_access_token()
                        .await
                        .map_err(|_| failure(unavailable()))?
                }
            };
            changed |= current != *sent;
            self.authorization
                .remember(Secret::new(current))
                .map_err(failure)?;
        }
        for (target, sent) in &prepared.sent {
            if target.method == Kind::Secret {
                self.authorization
                    .ingress
                    .save_connector_grant(target.id, target.revision, None, None)
                    .await
                    .map_err(failure)?;
                continue;
            }
            let token = self
                .authorization
                .ingress
                .plugins
                .connectors
                .access(
                    target,
                    &self.authorization.ingress,
                    Some(sent),
                    &self.authorization.stop,
                )
                .await
                .map_err(failure)?;
            changed |= token != *sent;
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
            self.rejected(&prepared).await?;
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
        if unauthorized(&result) && self.rejected(&prepared).await? {
            let fresh = self.prepare(uri, headers).await?;
            return self
                .inner
                .get_stream_with_max_sse_event_size(
                    fresh.uri,
                    session_id,
                    last_event_id,
                    fresh.token,
                    fresh.headers,
                    max,
                )
                .await
                .map_err(|_| failure(unavailable()));
        }
        result.map_err(|_| failure(unavailable()))
    }
}

#[cfg(test)]
mod tests;
