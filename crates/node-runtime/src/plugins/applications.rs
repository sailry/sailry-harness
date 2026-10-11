//! Finite application providers exchange protected settings only at their fixed endpoints.
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    plugin::{
        Reference,
        settings::{Binding, Provider},
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::Mutex as AsyncMutex, time::Instant};
mod transport;
pub(crate) use transport::Client;

#[derive(Clone)]
pub(crate) struct Target {
    provider: Provider,
    id: String,
    secret: Secret,
    pub(crate) header: HeaderName,
    origin: url::Origin,
    key: String,
}

impl Target {
    pub(crate) fn resolve(
        reference: &Reference,
        binding: &Binding,
        settings: &super::settings::Resolved,
        endpoint: &str,
        secret: &Secret,
    ) -> Result<Self, Fault> {
        let declaration = binding.application.as_ref().ok_or_else(unavailable)?;
        let id = settings
            .values
            .get(&declaration.client_id)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 2048 && !id.chars().any(char::is_control))
            .ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "application ID is not configured")
            })?;
        let endpoint = Url::parse(endpoint).map_err(|_| unavailable())?;
        let permitted = match declaration.provider {
            Provider::Feishu => endpoint.as_str() == "https://mcp.feishu.cn/mcp",
            Provider::Dingtalk => {
                endpoint.origin().ascii_serialization() == "https://api.dingtalk.com"
            }
        };
        if !permitted {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "application authentication target changed",
            ));
        }
        let header = HeaderName::from_bytes(
            binding
                .header
                .as_deref()
                .ok_or_else(unavailable)?
                .as_bytes(),
        )
        .map_err(|_| unavailable())?;
        let mut hash = blake3::Hasher::new();
        let revision = reference.settings_revision.to_string();
        let provider = format!("{:?}", declaration.provider);
        let origin = endpoint.origin().ascii_serialization();
        for value in [
            reference.name.as_str(),
            reference.digest.as_str(),
            revision.as_str(),
            provider.as_str(),
            id,
            secret.expose(),
            header.as_str(),
            origin.as_str(),
        ] {
            hash.update(value.as_bytes());
            hash.update(&[0]);
        }
        Ok(Self {
            provider: declaration.provider,
            id: id.into(),
            secret: secret.clone(),
            header,
            origin: endpoint.origin(),
            key: hash.finalize().to_hex().to_string(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Authorization {
    pub(super) service: Arc<Service>,
    pub(super) targets: Vec<Target>,
}

impl Authorization {
    pub(crate) fn new(service: Arc<Service>, targets: Vec<Target>) -> Option<Self> {
        (!targets.is_empty()).then_some(Self { service, targets })
    }
    pub(crate) fn client(self, inner: reqwest::Client) -> Client {
        Client::new(inner, self)
    }
}

struct Token {
    value: Secret,
    refresh_at: Instant,
}
type Cell = Arc<AsyncMutex<Option<Token>>>;

#[derive(Default)]
pub(crate) struct Service {
    client: OnceLock<Result<reqwest::Client, Fault>>,
    tokens: Mutex<BTreeMap<String, Cell>>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<Provider, (Url, Url)>>,
}

impl Service {
    fn client(&self) -> Result<&reqwest::Client, Fault> {
        // Node bootstrap selects the TLS provider before the first network request.
        self.client
            .get_or_init(|| {
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .timeout(Duration::from_secs(20))
                    .build()
                    .map_err(|_| unavailable())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub(crate) async fn access(&self, target: &Target) -> Result<HeaderValue, Fault> {
        let cell = {
            let mut tokens = self.tokens.lock().expect("application token lock is valid");
            tokens.retain(|_, cell| {
                Arc::strong_count(cell) > 1
                    || match cell.try_lock() {
                        Ok(state) => state
                            .as_ref()
                            .is_some_and(|token| token.refresh_at > Instant::now()),
                        Err(_) => true,
                    }
            });
            if tokens.len()
                >= sailry_protocol::plugin::MAX_INSTALLED
                    * sailry_protocol::plugin::settings::MAX_FIELDS
                && !tokens.contains_key(&target.key)
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "application token capacity is exhausted",
                ));
            }
            tokens.entry(target.key.clone()).or_default().clone()
        };
        let mut state = cell.lock().await;
        if state
            .as_ref()
            .is_none_or(|token| token.refresh_at <= Instant::now())
        {
            *state = Some(self.fetch(target).await?);
        }
        let mut value = HeaderValue::from_str(
            state
                .as_ref()
                .expect("application token is ready")
                .value
                .expose(),
        )
        .map_err(|_| unavailable())?;
        value.set_sensitive(true);
        Ok(value)
    }

    async fn fetch(&self, target: &Target) -> Result<Token, Fault> {
        let endpoint = self.token_endpoint(target.provider);
        let body = match target.provider {
            Provider::Feishu => json!({"app_id":target.id,"app_secret":target.secret.expose()}),
            Provider::Dingtalk => json!({"appKey":target.id,"appSecret":target.secret.expose()}),
        };
        let started = Instant::now();
        let mut response = self
            .client()?
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                format!(
                    "application token request failed (HTTP {})",
                    response.status().as_u16()
                ),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len().saturating_add(chunk.len()) > 32 * 1024 {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        let (token, expires) = match target.provider {
            Provider::Feishu => {
                let code = value
                    .get("code")
                    .and_then(Value::as_i64)
                    .ok_or_else(unavailable)?;
                if code != 0 {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        format!("application token request failed (provider code {code})"),
                    ));
                }
                (&value["tenant_access_token"], &value["expire"])
            }
            Provider::Dingtalk => (&value["accessToken"], &value["expireIn"]),
        };
        let token = token
            .as_str()
            .filter(|token| !token.is_empty() && token.len() <= 8192)
            .ok_or_else(unavailable)?;
        HeaderValue::from_str(token).map_err(|_| unavailable())?;
        let expires = expires
            .as_u64()
            .filter(|seconds| *seconds > 0)
            .ok_or_else(unavailable)?;
        let duration = Duration::from_secs(expires);
        let margin = (duration / 10).min(Duration::from_secs(30));
        let refresh_at = started
            .checked_add(duration - margin)
            .ok_or_else(unavailable)?;
        if refresh_at <= Instant::now() {
            return Err(unavailable());
        }
        Ok(Token {
            value: Secret::new(token.into()),
            refresh_at,
        })
    }

    fn token_endpoint(&self, provider: Provider) -> Url {
        #[cfg(any(test, feature = "test-support"))]
        if let Some((endpoint, _)) = self
            .fixtures
            .lock()
            .expect("application fixture lock is valid")
            .get(&provider)
        {
            return endpoint.clone();
        }
        Url::parse(match provider {
            Provider::Feishu => {
                "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
            }
            Provider::Dingtalk => "https://api.dingtalk.com/v1.0/oauth2/accessToken",
        })
        .expect("fixed application endpoint is valid")
    }

    pub(crate) fn resource(&self, target: &Target, uri: &str) -> Result<String, Fault> {
        let url = Url::parse(uri).map_err(|_| unavailable())?;
        if url.origin() != target.origin {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "application token belongs to another origin",
            ));
        }
        #[cfg(any(test, feature = "test-support"))]
        if let Some((_, origin)) = self
            .fixtures
            .lock()
            .expect("application fixture lock is valid")
            .get(&target.provider)
        {
            let mut mapped = origin.clone();
            mapped.set_path(url.path());
            mapped.set_query(url.query());
            return Ok(mapped.into());
        }
        Ok(uri.into())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, provider: Provider, token: &str, resource: &str) {
        let token = Url::parse(token).expect("fixture token endpoint is valid");
        let resource = Url::parse(resource).expect("fixture resource endpoint is valid");
        for endpoint in [&token, &resource] {
            assert!(
                endpoint.scheme() == "http"
                    && endpoint.host_str() == Some("127.0.0.1")
                    && endpoint.port().is_some()
            );
        }
        self.fixtures
            .lock()
            .expect("application fixture lock is valid")
            .insert(provider, (token, resource));
    }
}

pub(crate) fn validate(
    provider: Provider,
    binding: &Binding,
    servers: &BTreeMap<String, super::mcp::Server>,
) -> Result<(), Fault> {
    if binding.env.is_some() || binding.oauth.is_some() || binding.prefix.is_some() {
        return Err(Fault::invalid(
            "application credentials require a fixed provider header",
        ));
    }
    let endpoint = match (binding.server.as_deref(), binding.origin.as_deref()) {
        (Some(name), None) => match servers.get(name) {
            Some(super::mcp::Server::Http { url, .. } | super::mcp::Server::Sse { url, .. }) => {
                url.as_str()
            }
            _ => {
                return Err(Fault::invalid(
                    "application credentials require an HTTP MCP server",
                ));
            }
        },
        (None, Some(origin)) => origin,
        _ => return Err(Fault::invalid("application credentials require one target")),
    };
    let url = Url::parse(endpoint).map_err(|_| unavailable())?;
    let permitted = match provider {
        Provider::Feishu => binding.server.is_some() && endpoint == "https://mcp.feishu.cn/mcp",
        Provider::Dingtalk => {
            binding.origin.is_some()
                && url.origin().ascii_serialization() == "https://api.dingtalk.com"
        }
    };
    let header = match provider {
        Provider::Feishu => "x-lark-mcp-tat",
        Provider::Dingtalk => "x-acs-dingtalk-access-token",
    };
    if !permitted
        || !binding
            .header
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(header))
    {
        return Err(Fault::invalid(
            "application credentials must use the provider's fixed target and header",
        ));
    }
    Ok(())
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "application token request failed")
}

#[cfg(test)]
mod tests;
