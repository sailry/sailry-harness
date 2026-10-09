use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use sailry_protocol::{
    Secret,
    plugin::oauth::{Configuration, Provider},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub(crate) declaration: Declaration,
    pub(crate) configuration: Configuration,
    pub(crate) access_token: Secret,
    pub(crate) refresh_token: Option<Secret>,
    pub(crate) expires_at_ms: u64,
}

pub(crate) struct Flow {
    declaration: Declaration,
    configuration: Configuration,
    redirect: Url,
    state: Secret,
    verifier: Secret,
    url: String,
}

impl Flow {
    pub(crate) fn begin(
        declaration: Declaration,
        configuration: Configuration,
        redirect: &str,
    ) -> Result<Self, Fault> {
        let redirect = self::redirect(redirect)?;
        let (mut authorization, _) = providers::endpoints(declaration.provider, &configuration)?;
        let state = random();
        let verifier = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.expose().as_bytes()));
        let scopes = providers::scopes(&declaration)?;
        let mut query = authorization.query_pairs_mut();
        query.extend_pairs([
            ("response_type", "code"),
            ("client_id", &configuration.client_id),
            ("redirect_uri", redirect.as_str()),
            ("state", state.expose()),
            ("scope", &scopes),
        ]);
        if declaration.provider == Provider::Dingtalk {
            query.append_pair("prompt", "consent");
        } else {
            query
                .append_pair("code_challenge_method", "S256")
                .append_pair("code_challenge", &challenge);
        }
        if declaration.provider == Provider::Google {
            query
                .append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
        drop(query);
        Ok(Self {
            declaration,
            configuration,
            redirect,
            state,
            verifier,
            url: authorization.into(),
        })
    }

    pub(crate) fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn callback(&self, callback: &Secret) -> Result<Secret, Fault> {
        let invalid = || Fault::invalid("plugin OAuth callback does not match its attempt");
        if callback.expose().len() > 16 * 1024 || callback.expose().chars().any(char::is_control) {
            return Err(invalid());
        }
        let url = Url::parse(callback.expose()).map_err(|_| invalid())?;
        let mut base = url.clone();
        base.set_query(None);
        if url.fragment().is_some() || base != self.redirect {
            return Err(invalid());
        }
        let mut fields = BTreeMap::new();
        for (name, value) in url.query_pairs() {
            if fields
                .insert(name.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(invalid());
            }
        }
        if fields.get("state").map(String::as_str) != Some(self.state.expose())
            || fields.contains_key("error")
        {
            return Err(invalid());
        }
        let code = fields
            .remove("code")
            .filter(|value| {
                !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
            })
            .ok_or_else(invalid)?;
        Ok(Secret::new(code))
    }

    pub(crate) async fn complete(
        &self,
        service: &Service,
        callback: &Secret,
    ) -> Result<Grant, Fault> {
        let code = self.callback(callback)?;
        service
            .exchange(
                &self.declaration,
                &self.configuration,
                &code,
                Some((&self.redirect, &self.verifier)),
                None,
            )
            .await
    }
}

pub(crate) fn redirect(value: &str) -> Result<Url, Fault> {
    let invalid = || Fault::invalid("plugin OAuth redirect must use a controller loopback address");
    let url = Url::parse(value).map_err(|_| invalid())?;
    if value.len() > 2048
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
        || url.port().is_none()
        || url.path() == "/"
    {
        return Err(invalid());
    }
    Ok(url)
}

pub(super) fn now() -> Result<u64, Fault> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| unavailable())?
            .as_millis(),
    )
    .map_err(|_| unavailable())
}

fn random() -> Secret {
    let mut bytes = [0; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    Secret::new(URL_SAFE_NO_PAD.encode(bytes))
}
