use super::*;

#[derive(Deserialize)]
pub(super) struct Tokens {
    access_token: Secret,
    refresh_token: Option<Secret>,
    pub id_token: Option<Secret>,
    token_type: Option<String>,
    expires_in: u64,
}

impl Tokens {
    pub(super) fn grant(
        self,
        identity: identity::Identity,
        previous: Option<&Grant>,
    ) -> Result<Grant, Fault> {
        if self.expires_in == 0
            || self
                .token_type
                .as_deref()
                .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
        {
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
        let (previous_id, previous_refresh) = match previous {
            Some(Grant::ChatGptOAuth {
                id_token, refresh, ..
            }) => (Some(id_token), Some(refresh)),
            None => (None, None),
            Some(_) => {
                return Err(sailry_protocol::Fault::invalid(
                    "Codex authorization expected",
                ));
            }
        };
        let grant = Grant::ChatGptOAuth {
            access: self.access_token,
            refresh: self
                .refresh_token
                .or_else(|| previous_refresh.cloned())
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("authorization response has no refresh token")
                })?,
            expires_at_ms,
            subject: identity.sub,
            account_id: identity.account_id.ok_or_else(|| {
                sailry_protocol::Fault::invalid("authorization identity has no workspace")
            })?,
            email: identity.email,
            id_token: self
                .id_token
                .or_else(|| previous_id.cloned())
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("authorization response has no identity token")
                })?,
            renewal_pending: false,
        };
        grant.validate(Authentication::ChatGptOAuth)?;
        if previous.is_some_and(|previous| !grant.same_account(previous)) {
            return Err(sailry_protocol::Fault::invalid(
                "authorization refresh changed its account",
            ));
        }
        Ok(grant)
    }
}
