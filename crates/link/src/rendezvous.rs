//! Short-code exchange only. Endpoint authentication and trust remain in Link.
mod sharing;
pub use sharing::ShareState;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{Client, Url, redirect::Policy};
use sailry_protocol::{ErrorCode, Fault};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{EndpointAddr, Invitation, LinkHandle};

pub const DEFAULT_SERVICE: &str = "https://link.sailry.dev";

const MAX_RESPONSE: usize = 12 * 1024;

#[derive(Clone)]
pub struct Relay {
    client: Client,
    base: Url,
}

/// Retain this ID for retries; generating another ID starts a different operation.
#[derive(Clone, Serialize)]
pub struct RequestId(String);

impl Default for RequestId {
    fn default() -> Self {
        let random = iroh::SecretKey::generate().to_bytes();
        // A fixed generation permits safe native-object cleanup across retries.
        // Keep 208 random bits; this is not the Node command request ID format.
        let timestamp = now_ms().unwrap_or(0);
        let suffix: String = random[6..]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self(format!("{timestamp:012x}{suffix}"))
    }
}

/// No Debug implementation: the cancellation token is a bearer credential.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Code {
    pub code: String,
    pub expires_at_ms: u64,
    cancel_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Redemption {
    ticket: String,
    expires_at_ms: u64,
}

impl Relay {
    /// HTTPS is required except for literal loopback addresses used in local tests.
    pub fn new(address: &str) -> Result<Self, Fault> {
        let base = Url::parse(address).map_err(|_| invalid())?;
        let loopback = base.host_str().is_some_and(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
        if !(base.scheme() == "https" || base.scheme() == "http" && loopback)
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(invalid());
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(exchange_error)?;
        Ok(Self { client, base })
    }

    pub async fn publish(
        &self,
        invitation: &Invitation,
        request_id: &RequestId,
    ) -> Result<Code, Fault> {
        if invitation.expires_at_ms() <= now_ms()? {
            return Err(expired());
        }
        let mut code: Code = self
            .post(
                "v1/codes",
                &serde_json::json!({
                    "request_id": request_id, "ticket": invitation.ticket(),
                    "expires_at_ms": invitation.expires_at_ms(),
                }),
                201,
            )
            .await?;
        if !valid_code(&code.code) || !valid_token(&code.cancel_token) {
            return Err(invalid());
        }
        // Network latency must never extend the underlying Rust invitation.
        code.expires_at_ms = code.expires_at_ms.min(invitation.expires_at_ms());
        if code.expires_at_ms <= now_ms()? {
            return Err(expired());
        }
        Ok(code)
    }

    pub async fn pair(
        &self,
        link: &LinkHandle,
        code: &str,
        request_id: &RequestId,
    ) -> Result<EndpointAddr, Fault> {
        if !valid_code(code) {
            return Err(invalid());
        }
        let redemption: Redemption = self
            .post(
                "v1/codes/redeem",
                &serde_json::json!({
                    "request_id": request_id, "code": code,
                }),
                200,
            )
            .await?;
        if redemption.expires_at_ms <= now_ms()? {
            return Err(expired());
        }
        link.pair(&redemption.ticket).await
    }

    pub async fn cancel(&self, code: &Code) -> Result<(), Fault> {
        self.post::<serde_json::Value>(
            "v1/codes/cancel",
            &serde_json::json!({
                "code": code.code, "cancel_token": code.cancel_token,
            }),
            204,
        )
        .await
        .map(|_| ())
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
        expected: u16,
    ) -> Result<T, Fault> {
        let url = self.base.join(path).map_err(|_| invalid())?;
        let mut response = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(exchange_error)?;
        if response.status().as_u16() != expected {
            return Err(match response.status().as_u16() {
                404 | 410 => expired(),
                409 => Fault::new(ErrorCode::Conflict, "pairing request conflict"),
                429 => Fault::new(ErrorCode::Busy, "pairing service rate limit"),
                status => Fault::new(
                    ErrorCode::Unavailable,
                    format!("pairing service returned HTTP {status}"),
                ),
            });
        }
        if expected == 204 {
            return serde_json::from_str("null").map_err(|_| invalid());
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(exchange_error)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| invalid())
    }
}

fn valid_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit())
}
fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn now_ms() -> Result<u64, Fault> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .map_err(|_| unavailable())
}
fn invalid() -> Fault {
    Fault::new(ErrorCode::InvalidRequest, "invalid pairing exchange")
}
fn expired() -> Fault {
    Fault::new(ErrorCode::NotFound, "pairing code is unavailable")
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "pairing exchange is unavailable")
}

fn exchange_error(error: reqwest::Error) -> Fault {
    // URLs and request bodies may contain private deployment details.
    Fault::new(
        ErrorCode::Unavailable,
        format!("pairing service request failed: {:?}", error.without_url()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_origins() {
        for url in [
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com/?token=x",
            "https://example.com/#secret",
            "https://example.com/path",
        ] {
            assert!(Relay::new(url).is_err());
        }
        for url in [
            "https://example.com",
            "http://127.0.0.1:1234",
            "http://[::1]:1234",
        ] {
            assert!(Relay::new(url).is_ok());
        }
    }

    #[test]
    fn preserves_codes_and_retry_ids() {
        assert!(valid_code("001234"));
        for code in ["12345", "1234567", "１２３４５６", " 12345"] {
            assert!(!valid_code(code));
        }
        let first = RequestId::default();
        assert!(valid_token(&first.0));
        let created = u64::from_str_radix(&first.0[..12], 16).unwrap();
        assert!(now_ms().unwrap().saturating_sub(created) < 1000);
        assert_ne!(first.0, RequestId::default().0);
    }
}
