//! Public-client ChatGPT authorization and token renewal for the execution Node.
use super::*;
pub(super) mod flow;
pub(super) mod identity;
mod tokens;
pub(crate) use flow::Authorization;

use tokens::Tokens;

const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

impl Service {
    pub(super) async fn chatgpt<P, F>(
        &self,
        client: &Client,
        authorization: Authorization,
        progress: P,
    ) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let Authorization {
            redirect,
            host_id,
            previous,
            registration,
            issued,
            callback,
        } = authorization;
        if let Some(previous) = &previous {
            previous.validate(Authentication::ChatGpt)?;
        }
        let pending = flow::Pending::new(redirect, host_id, previous.as_ref(), registration)?;
        progress(State::Browser {
            authorization_url: pending.url(&self.auth)?,
            expires_at_ms: now_ms()? + DEADLINE.as_millis() as u64,
        })
        .await?;
        let callback = callback.await.map_err(|_| cancelled())?;
        let code = pending.callback(&callback)?;
        // An issued client belongs to this verified callback even if the code
        // expires during exchange. Persist it privately for explicit retries.
        let (acknowledge, stored) = tokio::sync::oneshot::channel();
        issued
            .send((code.client_id.clone(), acknowledge))
            .map_err(|_| cancelled())?;
        stored.await.map_err(|_| cancelled())??;
        let tokens: Tokens = json(
            client
                .post(format!("{}/api/accounts/oauth/token", self.auth))
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", code.client_id.as_str()),
                    ("redirect_uri", pending.redirect.as_str()),
                    ("resource", RESOURCE),
                    ("code", code.value.expose()),
                    ("code_verifier", pending.verifier.expose()),
                ]),
        )
        .await?;
        let id_token = tokens.id_token.as_ref().ok_or_else(|| {
            sailry_protocol::Fault::invalid("authorization response has no identity token")
        })?;
        let identity = identity::verify(
            client,
            &self.auth,
            id_token,
            &code.client_id,
            Some(pending.nonce.expose()),
        )
        .await?;
        if let Some(Grant::ChatGpt { subject, .. }) = &previous
            && identity.sub != *subject
        {
            return Err(sailry_protocol::Fault::invalid(
                "authorization changed the selected account",
            ));
        }
        tokens.grant(code.client_id, identity, None)
    }

    pub(super) async fn refresh_chatgpt(
        &self,
        client: &Client,
        previous: &Grant,
    ) -> Result<Grant, Fault> {
        previous.validate(Authentication::ChatGpt)?;
        let Grant::ChatGpt {
            client_id,
            subject,
            email,
            refresh,
            ..
        } = previous
        else {
            return Err(sailry_protocol::Fault::invalid(
                "ChatGPT authorization expected",
            ));
        };
        let tokens: Tokens = json(
            client
                .post(format!("{}/api/accounts/oauth/token", self.auth))
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id.as_str()),
                    ("refresh_token", refresh.expose()),
                    ("resource", RESOURCE),
                ]),
        )
        .await?;
        let identity = if let Some(id_token) = &tokens.id_token {
            let identity = identity::verify(client, &self.auth, id_token, client_id, None).await?;
            if identity.sub != *subject {
                return Err(sailry_protocol::Fault::invalid(
                    "authorization refresh changed its account",
                ));
            }
            identity
        } else {
            identity::Identity {
                sub: subject.clone(),
                email: email.clone(),
                account_id: None,
            }
        };
        tokens.grant(client_id.clone(), identity, Some(previous))
    }
}
