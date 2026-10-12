//! Fixed application registration follows the official Feishu SDK flow.
//! Reference: larksuite/oapi-sdk-python cc359bb7, scene/registration; protocol only,
//! not a copied SDK or a replacement OAuth engine.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, Url};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    plugin::{
        registration::Declaration,
        settings::{Provider, Schema},
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub(crate) struct Service {
    client: OnceLock<Result<Client, Fault>>,
    #[cfg(any(test, feature = "test-support"))]
    endpoint: std::sync::Mutex<Option<Url>>,
}

pub(crate) struct Credentials {
    pub(crate) declaration: Declaration,
    pub(crate) client_id: String,
    pub(crate) client_secret: Secret,
}

pub(crate) fn validate(declaration: &Declaration, schema: Option<&Schema>) -> Result<(), Fault> {
    let schema = schema
        .filter(|_| declaration.valid())
        .ok_or_else(|| Fault::invalid("invalid application registration declaration"))?;
    let id = schema
        .properties
        .get(&declaration.client_id)
        .filter(|field| field.secret.is_none())
        .ok_or_else(|| Fault::invalid("application registration ID is not a public setting"))?;
    if id.kind != sailry_protocol::plugin::settings::Kind::String {
        return Err(Fault::invalid(
            "application registration ID must be a string",
        ));
    }
    let binding = schema
        .properties
        .get(&declaration.client_secret)
        .and_then(|field| field.secret.as_ref())
        .ok_or_else(|| {
            Fault::invalid("application registration secret is not a protected setting")
        })?;
    if !binding.application.as_ref().is_some_and(|application| {
        application.provider == Provider::Feishu && application.client_id == declaration.client_id
    }) {
        return Err(Fault::invalid("application registration binding changed"));
    }
    Ok(())
}

impl Service {
    fn client(&self) -> Result<&Client, Fault> {
        self.client
            .get_or_init(|| {
                Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .retry(reqwest::retry::never())
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|_| unavailable())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn endpoint(&self) -> Result<Url, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(endpoint) = self.endpoint.lock().map_err(|_| unavailable())?.clone() {
            return Ok(endpoint);
        }
        Url::parse("https://accounts.feishu.cn/oauth/v1/app/registration")
            .map_err(|_| unavailable())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, endpoint: &str) {
        let endpoint = Url::parse(endpoint).expect("registration fixture URL is valid");
        assert!(
            endpoint.scheme() == "http"
                && endpoint.host_str() == Some("127.0.0.1")
                && endpoint.port().is_some()
        );
        *self.endpoint.lock().unwrap() = Some(endpoint);
    }

    async fn post(&self, fields: &[(&str, &str)]) -> Result<Value, Fault> {
        let mut response = self
            .client()?
            .post(self.endpoint()?)
            .form(fields)
            .send()
            .await
            .map_err(|_| unavailable())?;
        // Device-grant errors may use HTTP 400. Never expose provider bodies.
        if !response.status().is_success() && response.status() != reqwest::StatusCode::BAD_REQUEST
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
        serde_json::from_slice(&bytes).map_err(|_| unavailable())
    }

    pub(crate) async fn authorize<F, Fut>(
        &self,
        declaration: Declaration,
        prompt: F,
        stop: &CancellationToken,
    ) -> Result<Credentials, Fault>
    where
        F: FnOnce(String, u64) -> Fut,
        Fut: std::future::Future<Output = Result<(), Fault>>,
    {
        let operation = async {
            let initialized = self.post(&[("action", "init")]).await?;
            if !initialized["supported_auth_methods"]
                .as_array()
                .is_some_and(|methods| methods.iter().any(|method| method == "client_secret"))
            {
                return Err(unavailable());
            }
            let started = self
                .post(&[
                    ("action", "begin"),
                    ("archetype", "PersonalAgent"),
                    ("auth_method", "client_secret"),
                    ("request_user_info", "open_id"),
                ])
                .await?;
            let code = token(&started["device_code"])?;
            let mut interval = started
                .get("interval")
                .map(Value::as_u64)
                .unwrap_or(Some(5))
                .filter(|seconds| (1..=60).contains(seconds))
                .ok_or_else(unavailable)?;
            let seconds = started
                .get("expires_in")
                .map(Value::as_u64)
                .unwrap_or(Some(600))
                .filter(|seconds| (1..=900).contains(seconds))
                .ok_or_else(unavailable)?;
            let expires_at_ms = now()?.checked_add(seconds * 1000).ok_or_else(unavailable)?;
            let url = verification(&started["verification_uri_complete"], &declaration)?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
            prompt(url, expires_at_ms).await?;
            loop {
                if tokio::time::Instant::now() >= deadline {
                    return Err(expired());
                }
                let data = tokio::time::timeout_at(
                    deadline,
                    self.post(&[("action", "poll"), ("device_code", code.expose())]),
                )
                .await
                .map_err(|_| expired())??;
                if data
                    .pointer("/user_info/tenant_brand")
                    .and_then(Value::as_str)
                    .is_some_and(|brand| brand != "feishu")
                {
                    return Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "this application binding supports Feishu tenants only",
                    ));
                }
                if data.get("client_id").is_some() && data.get("client_secret").is_some() {
                    let client_id = token(&data["client_id"])?;
                    if !client_id.expose().starts_with("cli_") || client_id.expose().len() > 256 {
                        return Err(unavailable());
                    }
                    return Ok(Credentials {
                        declaration,
                        client_id: client_id.expose().into(),
                        client_secret: token(&data["client_secret"])?,
                    });
                }
                match data["error"].as_str() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => interval = interval.saturating_add(5).min(120),
                    Some("access_denied") => {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "application registration was declined",
                        ));
                    }
                    Some("expired_token") => return Err(expired()),
                    _ => return Err(unavailable()),
                }
                tokio::time::sleep_until(
                    (tokio::time::Instant::now() + Duration::from_secs(interval)).min(deadline),
                )
                .await;
            }
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "application registration observation cancelled")),
            result = tokio::time::timeout(Duration::from_secs(900), operation) => result.map_err(|_| expired())?,
        }
    }
}

fn verification(value: &Value, declaration: &Declaration) -> Result<String, Fault> {
    let value = value
        .as_str()
        .filter(|value| value.len() <= 4096)
        .ok_or_else(unavailable)?;
    let mut url = Url::parse(value).map_err(|_| unavailable())?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("accounts.feishu.cn" | "accounts.larksuite.com")
        )
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(unavailable());
    }
    let addons = json!({"preset":false, "scopes":{"tenant":declaration.tenant_scopes}});
    let bytes = serde_json::to_vec(&addons).map_err(|_| unavailable())?;
    let mut gzip = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    gzip.write_all(&bytes).map_err(|_| unavailable())?;
    let encoded = URL_SAFE_NO_PAD.encode(gzip.finish().map_err(|_| unavailable())?);
    // Replace registration options instead of accepting a provider-supplied broader preset.
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| {
            !matches!(
                key.as_ref(),
                "from"
                    | "tp"
                    | "source"
                    | "addons"
                    | "createOnly"
                    | "clientID"
                    | "name"
                    | "desc"
                    | "avatar"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(pairs)
        .append_pair("from", "sdk")
        .append_pair("tp", "sdk")
        .append_pair("source", "sailry")
        .append_pair("name", "Sailry")
        .append_pair("addons", &encoded)
        .append_pair("createOnly", "true");
    if url.as_str().len() > 8192 {
        return Err(unavailable());
    }
    Ok(url.into())
}

fn token(value: &Value) -> Result<Secret, Fault> {
    value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
        .map(|value| Secret::new(value.into()))
        .ok_or_else(unavailable)
}
fn now() -> Result<u64, Fault> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .ok_or_else(unavailable)
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "application registration outcome is unknown; check the provider before creating another application",
    )
}
fn expired() -> Fault {
    Fault::new(
        ErrorCode::Expired,
        "application registration expired; check the provider before creating another application",
    )
}

#[cfg(test)]
mod tests;
