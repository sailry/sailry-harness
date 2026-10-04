use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};

// Public OAuth application identifier used by the vendor's device authorization flow.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

#[derive(Deserialize)]
struct Device {
    device_auth_id: Secret,
    #[serde(alias = "usercode")]
    user_code: String,
    interval: Option<Interval>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Interval {
    Number(u64),
    Text(String),
}

impl Interval {
    fn seconds(self) -> Result<u64, Fault> {
        match self {
            Self::Number(value) => Ok(value),
            Self::Text(value) => value
                .parse()
                .map_err(|_| invalid("authorization poll interval is invalid")),
        }
    }
}

#[derive(Deserialize)]
struct Code {
    authorization_code: Secret,
    code_verifier: Secret,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: Secret,
    refresh_token: Option<Secret>,
    id_token: Option<Secret>,
}

impl Service {
    pub(super) async fn chatgpt<P, F>(&self, client: &Client, progress: P) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let device: Device = json(
            client
                .post(format!("{}/api/accounts/deviceauth/usercode", self.auth))
                .json(&json!({ "client_id": CLIENT_ID })),
        )
        .await?;
        let interval = Duration::from_secs(
            device
                .interval
                .map(Interval::seconds)
                .transpose()?
                .unwrap_or(5)
                .clamp(1, 900),
        );
        progress(prompt(
            "https://auth.openai.com/codex/device",
            device.user_code.clone(),
            DEADLINE,
        )?)
        .await?;
        loop {
            tokio::time::sleep(interval).await;
            let (status, body) = response(client.post(format!("{}/api/accounts/deviceauth/token", self.auth)).json(&json!({ "device_auth_id": device.device_auth_id.expose(), "user_code": device.user_code }))).await?;
            if matches!(status.as_u16(), 403 | 404) {
                continue;
            }
            if !status.is_success() {
                return Err(http_error(status));
            }
            let code: Code = decode(&body)?;
            let tokens: Tokens = json(client.post(format!("{}/oauth/token", self.auth)).form(&[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                (
                    "redirect_uri",
                    &format!("{}/deviceauth/callback", self.auth),
                ),
                ("code", code.authorization_code.expose()),
                ("code_verifier", code.code_verifier.expose()),
            ]))
            .await?;
            return tokens.grant(None);
        }
    }

    pub(super) async fn refresh_chatgpt(
        &self,
        client: &Client,
        previous: &Grant,
    ) -> Result<Grant, Fault> {
        let Grant::ChatGpt { refresh, .. } = previous else {
            return Err(invalid("ChatGPT authorization expected"));
        };
        let tokens: Tokens = json(client.post(format!("{}/oauth/token", self.auth)).form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh.expose()),
            ("scope", "openid profile email"),
        ]))
        .await?;
        tokens.grant(Some(previous))
    }
}

impl Tokens {
    fn grant(self, previous: Option<&Grant>) -> Result<Grant, Fault> {
        // Claims are vendor-returned expiry/routing metadata, never proof of local authentication.
        let access_claims = claims(&self.access_token)?;
        let expires_at_ms = access_claims
            .get("exp")
            .and_then(Value::as_u64)
            .and_then(|value| value.checked_mul(1000))
            .ok_or_else(|| invalid("authorization token has no valid expiry"))?;
        let id_claims = self.id_token.as_ref().map(claims).transpose()?;
        let account = |claims: &Value| {
            claims
                .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let account_id = id_claims
            .as_ref()
            .and_then(account)
            .or_else(|| account(&access_claims))
            .or_else(|| match previous {
                Some(Grant::ChatGpt { account_id, .. }) => Some(account_id.clone()),
                _ => None,
            })
            .ok_or_else(|| invalid("authorization token has no account identifier"))?;
        Ok(Grant::ChatGpt {
            access: self.access_token,
            refresh: self
                .refresh_token
                .or_else(|| match previous {
                    Some(Grant::ChatGpt { refresh, .. }) => Some(refresh.clone()),
                    _ => None,
                })
                .ok_or_else(|| invalid("authorization token has no renewal grant"))?,
            expires_at_ms,
            account_id,
            renewal_pending: false,
        })
    }
}

fn claims(token: &Secret) -> Result<Value, Fault> {
    let payload = token
        .expose()
        .split('.')
        .nth(1)
        .ok_or_else(|| invalid("authorization token has invalid claims"))?;
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| invalid("authorization token has invalid claims"))?,
    );
    decode(&decoded)
}
