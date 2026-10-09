use super::*;
use crate::store::Ingress;
use reqwest::{
    Client, Url,
    header::{HeaderName, HeaderValue},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    CredentialId, Secret,
    plugin::oauth::{Configuration, Provider},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct Target {
    pub(crate) plugin: String,
    pub(crate) id: CredentialId,
    pub(crate) declaration: Declaration,
    pub(crate) header: HeaderName,
    pub(crate) prefix: String,
}

pub(crate) struct Service {
    client: Result<Client, Fault>,
    locks: Mutex<BTreeMap<CredentialId, Weak<tokio::sync::Mutex<()>>>>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<Provider, Url>>,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| unavailable()),
            locks: Mutex::default(),
            #[cfg(any(test, feature = "test-support"))]
            fixtures: Mutex::default(),
        }
    }
}

impl Service {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, provider: Provider, url: &str) {
        self.fixtures
            .lock()
            .unwrap()
            .insert(provider, Url::parse(url).unwrap());
    }

    pub(super) async fn exchange(
        &self,
        declaration: &Declaration,
        configuration: &Configuration,
        value: &Secret,
        code: Option<(&Url, &Secret)>,
        previous: Option<&Grant>,
    ) -> Result<Grant, Fault> {
        let (_, endpoint) = providers::endpoints(declaration.provider, configuration)?;
        #[cfg(any(test, feature = "test-support"))]
        let endpoint = self
            .fixtures
            .lock()
            .unwrap()
            .get(&declaration.provider)
            .cloned()
            .unwrap_or(endpoint);
        let client = self.client.as_ref().map_err(Clone::clone)?;
        let grant_type = if code.is_some() {
            "authorization_code"
        } else {
            "refresh_token"
        };
        let mut request = client.post(endpoint);
        if declaration.provider == Provider::Dingtalk {
            let mut body = json!({"clientId": configuration.client_id,
                "clientSecret": configuration.client_secret.as_ref().map(Secret::expose), "grantType": grant_type});
            body[if code.is_some() {
                "code"
            } else {
                "refreshToken"
            }] = Value::String(value.expose().into());
            request = request.json(&body);
        } else {
            let mut form = vec![
                ("client_id", configuration.client_id.as_str()),
                ("grant_type", grant_type),
            ];
            if let Some(secret) = &configuration.client_secret {
                form.push(("client_secret", secret.expose()));
            }
            if let Some((redirect, verifier)) = code {
                form.extend([
                    ("code", value.expose()),
                    ("redirect_uri", redirect.as_str()),
                    ("code_verifier", verifier.expose()),
                ]);
            } else {
                form.push(("refresh_token", value.expose()));
            }
            request = request.form(&form);
        }
        let mut response = request.send().await.map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        if response
            .content_length()
            .is_some_and(|length| length > 64 * 1024)
        {
            return Err(unavailable());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len().saturating_add(chunk.len()) > 64 * 1024 {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        let ding = declaration.provider == Provider::Dingtalk;
        let access_token = token(&body[if ding { "accessToken" } else { "access_token" }])?;
        if !ding
            && body["token_type"]
                .as_str()
                .is_none_or(|kind| !kind.eq_ignore_ascii_case("Bearer"))
        {
            return Err(unavailable());
        }
        let expires = body[if ding { "expireIn" } else { "expires_in" }]
            .as_u64()
            .filter(|seconds| (1..=31_536_000).contains(seconds))
            .ok_or_else(unavailable)?;
        let refresh = &body[if ding {
            "refreshToken"
        } else {
            "refresh_token"
        }];
        let refresh_token = if refresh.is_null() {
            previous.and_then(|grant| grant.refresh_token.clone())
        } else {
            Some(token(refresh)?)
        };
        Ok(Grant {
            declaration: declaration.clone(),
            configuration: configuration.clone(),
            access_token,
            refresh_token,
            expires_at_ms: flow::now()?
                .checked_add(expires * 1000)
                .ok_or_else(unavailable)?,
        })
    }

    pub(crate) async fn access(
        &self,
        ingress: &Ingress,
        target: &Target,
        stop: &CancellationToken,
    ) -> Result<HeaderValue, Fault> {
        let lock = {
            let mut locks = self.locks.lock().map_err(|_| unavailable())?;
            locks.retain(|_, lock| lock.strong_count() != 0);
            match locks.get(&target.id).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(tokio::sync::Mutex::new(()));
                    locks.insert(target.id, Arc::downgrade(&lock));
                    lock
                }
            }
        };
        let _guard = tokio::select! {
            _ = stop.cancelled() => return Err(cancelled()),
            guard = lock.lock() => guard,
        };
        let (revision, mut grant) = ingress.plugin_oauth_credentials(target.clone()).await?;
        if grant.declaration != target.declaration {
            return Err(unavailable());
        }
        if grant.expires_at_ms <= flow::now()?.saturating_add(30_000) {
            let refresh = grant.refresh_token.as_ref().ok_or_else(unavailable)?;
            grant = tokio::select! {
                _ = stop.cancelled() => return Err(cancelled()),
                result = self.exchange(&grant.declaration, &grant.configuration, refresh, None, Some(&grant)) => result?,
            };
            ingress
                .save_plugin_oauth_credentials(target.clone(), revision, grant.clone())
                .await?;
        }
        // Recheck the grant after refresh/revocation races before making the resource request.
        let (_, current) = ingress.plugin_oauth_credentials(target.clone()).await?;
        if current.declaration != target.declaration {
            return Err(unavailable());
        }
        let mut value = HeaderValue::from_str(&format!(
            "{}{}",
            target.prefix,
            current.access_token.expose()
        ))
        .map_err(|_| unavailable())?;
        value.set_sensitive(true);
        Ok(value)
    }
}

fn token(value: &Value) -> Result<Secret, Fault> {
    let value = value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
        .ok_or_else(unavailable)?;
    Ok(Secret::new(value.into()))
}

fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "plugin authorization cancelled")
}
