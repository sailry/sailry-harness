use super::*;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
pub(super) struct Tokens {
    access_token: Secret,
    refresh_token: Secret,
    pub id_token: Option<Secret>,
    token_type: String,
    expires_in: u64,
    earliest_refresh_at: Option<u64>,
    scope: Option<String>,
}

#[cfg(test)]
mod tests;

impl Tokens {
    pub(super) fn grant(
        self,
        client_id: String,
        identity: identity::Identity,
        previous: Option<&Grant>,
    ) -> Result<Grant, Fault> {
        if !self.token_type.eq_ignore_ascii_case("bearer") || self.expires_in == 0 {
            return Err(sailry_protocol::Fault::invalid(
                "authorization token response is invalid",
            ));
        }
        let expires_at_ms = self
            .expires_in
            .checked_mul(1000)
            .and_then(|duration| now_ms().ok()?.checked_add(duration))
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid("authorization token expiry is invalid")
            })?;
        let (previous_id, previous_scopes, previous_account) = match previous {
            Some(Grant::ChatGpt {
                id_token,
                scopes,
                account_id,
                ..
            }) => (Some(id_token), Some(scopes), account_id.clone()),
            None => (None, None, None),
            Some(_) => {
                return Err(sailry_protocol::Fault::invalid(
                    "ChatGPT authorization expected",
                ));
            }
        };
        let scopes: Vec<String> = match self.scope {
            Some(scope) => scope
                .split_ascii_whitespace()
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            None => previous_scopes.cloned().ok_or_else(|| {
                sailry_protocol::Fault::invalid("authorization response has no granted scopes")
            })?,
        };
        let grant = Grant::ChatGpt {
            client_id,
            subject: identity.sub,
            email: identity.email,
            account_id: identity.account_id.or(previous_account),
            id_token: self
                .id_token
                .or_else(|| previous_id.cloned())
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("authorization response has no identity token")
                })?,
            scopes,
            access: self.access_token,
            refresh: self.refresh_token,
            expires_at_ms,
            earliest_refresh_at: self.earliest_refresh_at,
            renewal_pending: false,
        };
        grant.validate(Authentication::ChatGpt)?;
        Ok(grant)
    }
}
