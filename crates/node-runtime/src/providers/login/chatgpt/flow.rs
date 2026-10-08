use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const REGISTRATION_CLIENT: &str = "dynamic_agent_client";

pub(crate) struct Authorization {
    pub redirect: String,
    pub host_id: String,
    pub previous: Option<Grant>,
    pub registration: Option<String>,
    pub issued:
        tokio::sync::oneshot::Sender<(String, tokio::sync::oneshot::Sender<Result<(), Fault>>)>,
    pub callback: tokio::sync::oneshot::Receiver<Secret>,
}

pub(super) struct Pending {
    pub redirect: String,
    pub verifier: Secret,
    pub nonce: Secret,
    state: Secret,
    host_id: String,
    client_id: Option<String>,
}

pub(super) struct Code {
    pub client_id: String,
    pub value: Secret,
}

impl Pending {
    pub(super) fn new(
        redirect: String,
        host_id: String,
        previous: Option<&Grant>,
        registration: Option<String>,
    ) -> Result<Self, Fault> {
        loopback(&redirect)?;
        if host_id.is_empty() || host_id.len() > 256 || host_id.chars().any(char::is_control) {
            return Err(sailry_protocol::Fault::invalid(
                "authorization host identifier is invalid",
            ));
        }
        let client_id = match previous {
            Some(Grant::ChatGpt { client_id, .. }) => {
                issued_client(client_id)?;
                Some(client_id.clone())
            }
            Some(_) => {
                return Err(sailry_protocol::Fault::invalid(
                    "ChatGPT authorization expected",
                ));
            }
            None => {
                if let Some(client_id) = &registration {
                    issued_client(client_id)?;
                }
                registration
            }
        };
        Ok(Self {
            redirect,
            host_id,
            client_id,
            state: random(),
            nonce: random(),
            verifier: random(),
        })
    }

    pub(super) fn url(&self, auth: &str) -> Result<String, Fault> {
        let mut url = Url::parse(&format!("{auth}/api/accounts/authorize"))
            .map_err(|_| sailry_protocol::Fault::invalid("authorization endpoint is invalid"))?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.expose().as_bytes()));
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            (
                "client_id",
                self.client_id.as_deref().unwrap_or(REGISTRATION_CLIENT),
            ),
            ("ext_agent_host_id", self.host_id.as_str()),
            ("redirect_uri", self.redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", self.state.expose()),
            ("nonce", self.nonce.expose()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
        ]);
        if self.client_id.is_none() {
            query.append_pair("agent_name_hint", "Sailry");
        }
        // Browser progress crosses the controller boundary. Account tokens stay
        // in the execution Node, so returning sign-in uses the account selector.
        drop(query);
        Ok(url.into())
    }

    pub(super) fn callback(&self, value: &Secret) -> Result<Code, Fault> {
        let url = Url::parse(value.expose())
            .map_err(|_| sailry_protocol::Fault::invalid("authorization callback is invalid"))?;
        let mut base = url.clone();
        base.set_query(None);
        if url.fragment().is_some() || base.as_str() != self.redirect {
            return Err(sailry_protocol::Fault::invalid(
                "authorization callback does not match its redirect",
            ));
        }
        let mut fields = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(sailry_protocol::Fault::invalid(
                    "authorization callback has duplicate parameters",
                ));
            }
        }
        if fields.get("state").map(String::as_str) != Some(self.state.expose()) {
            return Err(sailry_protocol::Fault::invalid(
                "authorization callback state did not match",
            ));
        }
        if fields.contains_key("error") {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "ChatGPT authorization was not granted",
            ));
        }
        let client_id = match (&self.client_id, fields.remove("client_id")) {
            (Some(previous), Some(returned)) if *previous != returned => {
                return Err(sailry_protocol::Fault::invalid(
                    "authorization changed the selected registration",
                ));
            }
            (Some(previous), _) => previous.clone(),
            (None, Some(returned)) => returned,
            (None, None) => {
                return Err(sailry_protocol::Fault::invalid(
                    "authorization registration is incomplete",
                ));
            }
        };
        issued_client(&client_id)?;
        let value = fields
            .remove("code")
            .filter(|value| !value.is_empty() && value.len() <= 16384)
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid("authorization callback has no valid code")
            })?;
        Ok(Code {
            client_id,
            value: Secret::new(value),
        })
    }
}

pub(in super::super) fn issued_client(value: &str) -> Result<(), Fault> {
    if value.is_empty()
        || value.len() > 256
        || value == REGISTRATION_CLIENT
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization client identifier is invalid",
        ));
    }
    Ok(())
}

fn loopback(value: &str) -> Result<(), Fault> {
    let url = Url::parse(value)
        .map_err(|_| sailry_protocol::Fault::invalid("authorization redirect is invalid"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none_or(|port| port == 0)
        || url.path() != "/auth/callback"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization requires an exact loopback callback",
        ));
    }
    Ok(())
}

fn random() -> Secret {
    let mut bytes = [0; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    Secret::new(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests;
