//! Pi-compatible Claude authorization; credentials remain on the execution Node.
//! Adapted from Pi 4270522473e677ddba7b093caa0064a3df51d459.
//! Copyright (c) 2025 Mario Zechner. MIT license: third_party_licenses/pi-MIT.txt.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const COPY_REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

struct Pending {
    redirect: String,
    verifier: Secret,
}

impl Pending {
    fn new(redirect: String) -> Result<Self, Fault> {
        if redirect != COPY_REDIRECT {
            let url = Url::parse(&redirect)
                .map_err(|_| Fault::invalid("authorization redirect is invalid"))?;
            if url.scheme() != "http"
                || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
                || url.port().is_none_or(|port| port == 0)
                || url.path() != "/callback"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.as_str() != redirect
            {
                return Err(Fault::invalid(
                    "authorization requires an exact loopback callback",
                ));
            }
        }
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Ok(Self {
            redirect,
            verifier: Secret::new(URL_SAFE_NO_PAD.encode(bytes)),
        })
    }

    fn url(&self, auth: &str) -> Result<String, Fault> {
        let mut url = Url::parse(&format!("{auth}/oauth/authorize"))
            .map_err(|_| Fault::invalid("authorization endpoint is invalid"))?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.expose().as_bytes()));
        url.query_pairs_mut().extend_pairs([
            ("code", "true"),
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", self.redirect.as_str()),
            ("scope", SCOPES),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
            ("state", self.verifier.expose()),
        ]);
        Ok(url.into())
    }

    fn code(&self, input: &Secret) -> Result<Secret, Fault> {
        let input = input.expose().trim();
        let (code, state) = if input.starts_with("http://") || input.starts_with("https://") {
            let url = Url::parse(input)
                .map_err(|_| Fault::invalid("authorization callback is invalid"))?;
            let mut base = url.clone();
            base.set_query(None);
            if url.fragment().is_some() || base.as_str() != self.redirect {
                return Err(Fault::invalid(
                    "authorization callback does not match its redirect",
                ));
            }
            parameters(url.query_pairs())?
        } else if input.contains("code=") {
            parameters(url::form_urlencoded::parse(input.as_bytes()))?
        } else if let Some((code, state)) = input.split_once('#') {
            (code.to_owned(), Some(state.to_owned()))
        } else {
            (input.to_owned(), None)
        };
        if state
            .as_deref()
            .is_some_and(|state| state != self.verifier.expose())
        {
            return Err(Fault::invalid("authorization callback state did not match"));
        }
        if code.is_empty()
            || code.len() > 16 * 1024
            || code.chars().any(char::is_whitespace)
            || code.chars().any(char::is_control)
        {
            return Err(Fault::invalid("authorization callback has no valid code"));
        }
        Ok(Secret::new(code))
    }
}

fn parameters<'a>(
    values: impl Iterator<Item = (std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>)>,
) -> Result<(String, Option<String>), Fault> {
    let mut fields = BTreeMap::new();
    for (name, value) in values {
        if fields
            .insert(name.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(Fault::invalid(
                "authorization callback has duplicate parameters",
            ));
        }
    }
    if fields.contains_key("error") {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Claude authorization was not granted",
        ));
    }
    Ok((
        fields.remove("code").unwrap_or_default(),
        fields.remove("state"),
    ))
}

#[derive(Deserialize)]
struct Tokens {
    access_token: Secret,
    refresh_token: Secret,
    expires_in: u64,
}

impl Tokens {
    fn grant(self) -> Result<Grant, Fault> {
        let lifetime = self
            .expires_in
            .checked_mul(1000)
            .and_then(|duration| now_ms().ok()?.checked_add(duration))
            .filter(|expiry| *expiry <= i64::MAX as u64)
            .ok_or_else(|| Fault::invalid("authorization expiry is invalid"))?;
        // Pi refreshes five minutes before expiry. The existing Node serializes
        // rotating grants and durably marks an in-flight renewal before HTTP.
        let grant = Grant::ClaudeOAuth {
            access: self.access_token,
            refresh: self.refresh_token,
            expires_at_ms: lifetime.saturating_sub(5 * 60 * 1000),
            renewal_pending: false,
        };
        grant.validate(Authentication::ClaudeOAuth)?;
        Ok(grant)
    }
}

impl Service {
    pub(super) async fn claude<P, F>(
        &self,
        client: &Client,
        authorization: Authorization,
        progress: P,
    ) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let pending = Pending::new(authorization.redirect)?;
        progress(State::Browser {
            authorization_url: pending.url(&self.claude_auth)?,
            expires_at_ms: now_ms()? + DEADLINE.as_millis() as u64,
        })
        .await?;
        let callback = authorization.callback.await.map_err(|_| cancelled())?;
        let code = pending.code(&callback)?;
        let tokens: Tokens = json(client.post(&self.claude_token).json(&json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "code": code.expose(),
            "state": pending.verifier.expose(),
            "redirect_uri": pending.redirect,
            "code_verifier": pending.verifier.expose(),
        })))
        .await?;
        tokens.grant()
    }

    pub(super) async fn refresh_claude(
        &self,
        client: &Client,
        grant: &Grant,
    ) -> Result<Grant, Fault> {
        let Grant::ClaudeOAuth { refresh, .. } = grant else {
            return Err(Fault::invalid("Claude authorization expected"));
        };
        // Scope is deliberately omitted, matching Pi's refresh contract.
        let tokens: Tokens = json(client.post(&self.claude_token).json(&json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh.expose(),
        })))
        .await?;
        tokens.grant()
    }
}

#[cfg(test)]
mod tests;
