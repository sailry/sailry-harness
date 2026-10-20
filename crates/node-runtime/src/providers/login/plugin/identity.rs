//! OIDC proof validation shared by package-declared account flows.
use super::super::*;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode_header, jwk::JwkSet};
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: Option<String>,
    nonce: Option<String>,
    iat: u64,
    aud: serde_json::Value,
    azp: Option<String>,
}

pub(super) async fn verify(
    client: &Client,
    auth: &str,
    token: &Secret,
    client_id: &str,
    nonce: Option<&str>,
) -> Result<Value, Fault> {
    let discovery: Discovery =
        json(client.get(format!("{auth}/.well-known/openid-configuration"))).await?;
    discovery.validate(auth)?;
    let keys: JwkSet = json(client.get(&discovery.jwks_uri)).await?;
    verified(token, client_id, nonce, &discovery.issuer, &keys)
}

impl Discovery {
    fn validate(&self, auth: &str) -> Result<(), Fault> {
        let issuer = Url::parse(auth)
            .map_err(|_| sailry_protocol::Fault::invalid("authorization issuer is invalid"))?;
        let keys = Url::parse(&self.jwks_uri).map_err(|_| {
            sailry_protocol::Fault::invalid("authorization signing endpoint is invalid")
        })?;
        let secure = issuer.scheme() == "https";
        #[cfg(any(test, feature = "test-support"))]
        let secure =
            secure || (issuer.scheme() == "http" && issuer.host_str() == Some("127.0.0.1"));
        if !secure
            || self.issuer != auth
            || keys.origin() != issuer.origin()
            || !keys.username().is_empty()
            || keys.password().is_some()
            || keys.fragment().is_some()
            || keys.query().is_some()
        {
            return Err(sailry_protocol::Fault::invalid(
                "authorization discovery did not match its issuer",
            ));
        }
        Ok(())
    }
}

fn verified(
    token: &Secret,
    client_id: &str,
    nonce: Option<&str>,
    issuer: &str,
    keys: &JwkSet,
) -> Result<Value, Fault> {
    let header = decode_header(token.expose())
        .map_err(|_| sailry_protocol::Fault::invalid("authorization identity token is invalid"))?;
    if !matches!(
        header.alg,
        Algorithm::RS256 | Algorithm::ES256 | Algorithm::EdDSA
    ) {
        return Err(sailry_protocol::Fault::invalid(
            "authorization identity algorithm is unsupported",
        ));
    }
    let key = header
        .kid
        .as_deref()
        .and_then(|kid| keys.find(kid))
        .ok_or_else(|| {
            sailry_protocol::Fault::invalid("authorization identity signing key was not found")
        })?;
    if key
        .common
        .key_algorithm
        .is_some_and(|algorithm| algorithm != header.alg.into())
        || key
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|usage| *usage != jsonwebtoken::jwk::PublicKeyUse::Signature)
        || key
            .common
            .key_operations
            .as_ref()
            .is_some_and(|operations| {
                !operations.contains(&jsonwebtoken::jwk::KeyOperations::Verify)
            })
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization identity signing key is invalid",
        ));
    }
    let key = DecodingKey::from_jwk(key).map_err(|_| {
        sailry_protocol::Fault::invalid("authorization identity signing key is invalid")
    })?;
    let mut validation = Validation::new(header.alg);
    validation.leeway = 5;
    validation.validate_nbf = true;
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[issuer]);
    validation.set_required_spec_claims(&["exp", "sub", "iss", "aud"]);
    let document = jsonwebtoken::decode::<Value>(token.expose(), &key, &validation)
        .map_err(|_| Fault::invalid("authorization identity token could not be verified"))?
        .claims;
    let claims: Claims = serde_json::from_value(document.clone())
        .map_err(|_| Fault::invalid("authorization identity claims are invalid"))?;
    let now = now_ms()? / 1000;
    if claims.sub.is_empty()
        || claims.sub.len() > 2048
        || claims.sub.chars().any(char::is_control)
        || claims.iat > now.saturating_add(5)
        || nonce.is_some_and(|expected| claims.nonce.as_deref() != Some(expected))
        || claims
            .azp
            .as_deref()
            .is_some_and(|authorized| authorized != client_id)
        || (claims
            .aud
            .as_array()
            .is_some_and(|audiences| audiences.len() > 1)
            && claims.azp.as_deref() != Some(client_id))
        || claims.email.as_ref().is_some_and(|email| {
            email.is_empty() || email.len() > 1024 || email.chars().any(char::is_control)
        })
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization identity claims did not match",
        ));
    }
    Ok(document)
}

#[cfg(test)]
mod tests;
