//! Fixed-client Codex device authorization, separate from SIWC registration.
use super::*;
use chatgpt::identity;
use serde::Deserialize;
use serde_json::{Value, json};

mod tokens;
use tokens::Tokens;

// Public-client constants and device/token flow follow reviewed CC Switch
// 5ae6ad3888ba4543f6fad343c87656a97bd69da4 and CLIProxyAPI
// 0f96f568e4dbf6f84ad7399a74b78344c5eac7e6. No SIWC registration is reused.
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";

#[derive(Deserialize)]
struct Device {
    device_auth_id: Secret,
    #[serde(alias = "usercode")]
    user_code: String,
    interval: Option<Value>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct Code {
    authorization_code: Secret,
    code_verifier: Secret,
}

impl Service {
    pub(super) async fn codex<P, F>(&self, client: &Client, progress: P) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let device: Device = document(
            client
                .post(format!("{}/api/accounts/deviceauth/usercode", self.auth))
                .json(&json!({"client_id": CLIENT_ID})),
            "device_code",
        )
        .await?;
        required(&device.device_auth_id)?;
        let duration = Duration::from_secs(device.expires_in.unwrap_or(900).min(900));
        if duration.is_zero() {
            return Err(expired());
        }
        let deadline = tokio::time::Instant::now() + duration;
        let mut interval = Duration::from_secs(interval(device.interval.as_ref()));
        progress(prompt(
            VERIFICATION_URL,
            device.user_code.clone(),
            duration,
        )?)
        .await?;
        tokio::time::timeout_at(deadline, async {
            loop {
                tokio::time::sleep(interval).await;
                let (status, body) = response(
                    client
                        .post(format!("{}/api/accounts/deviceauth/token", self.auth))
                        .json(&json!({
                            "device_auth_id": device.device_auth_id.expose(),
                            "user_code": device.user_code,
                        })),
                )
                .await
                .inspect_err(|error| {
                    diagnostic("device_poll", "transport", None, Some(error.code));
                })?;
                if let Some(pending) = pending(status, &body) {
                    diagnostic(
                        "device_poll",
                        match pending {
                            Poll::Pending => "pending",
                            Poll::SlowDown => "slow_down",
                        },
                        Some(status),
                        None,
                    );
                    if pending == Poll::SlowDown {
                        interval =
                            interval
                                .checked_add(Duration::from_secs(5))
                                .ok_or_else(|| {
                                    Fault::invalid("authorization polling interval overflowed")
                                })?;
                    }
                    continue;
                }
                diagnostic(
                    "device_poll",
                    if status.is_success() {
                        "http_success"
                    } else {
                        "http_rejected"
                    },
                    Some(status),
                    (!status.is_success()).then(|| {
                        if status == StatusCode::GONE {
                            ErrorCode::Expired
                        } else {
                            http_error(status).code
                        }
                    }),
                );
                if status == StatusCode::GONE {
                    return Err(expired());
                }
                if !status.is_success() {
                    return Err(http_error(status));
                }
                let code: Code = decode(&body).inspect_err(|error| {
                    diagnostic("device_poll", "decode", Some(status), Some(error.code));
                })?;
                required(&code.authorization_code)?;
                required(&code.code_verifier)?;
                let tokens: Tokens = document(
                    client.post(format!("{}/oauth/token", self.auth)).form(&[
                        ("grant_type", "authorization_code"),
                        ("client_id", CLIENT_ID),
                        ("redirect_uri", REDIRECT_URI),
                        ("code", code.authorization_code.expose()),
                        ("code_verifier", code.code_verifier.expose()),
                    ]),
                    "token_exchange",
                )
                .await?;
                return self.codex_grant(client, tokens, None).await;
            }
        })
        .await
        .map_err(|_| expired())?
    }

    pub(super) async fn refresh_codex(
        &self,
        client: &Client,
        previous: &Grant,
    ) -> Result<Grant, Fault> {
        previous.validate(Authentication::ChatGptOAuth)?;
        let Grant::ChatGptOAuth { refresh, .. } = previous else {
            return Err(sailry_protocol::Fault::invalid(
                "Codex authorization expected",
            ));
        };
        let tokens: Tokens = document(
            client.post(format!("{}/oauth/token", self.auth)).form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", refresh.expose()),
                ("scope", "openid profile email"),
            ]),
            "token_refresh",
        )
        .await?;
        self.codex_grant(client, tokens, Some(previous)).await
    }

    async fn codex_grant(
        &self,
        client: &Client,
        tokens: Tokens,
        previous: Option<&Grant>,
    ) -> Result<Grant, Fault> {
        let identity = match &tokens.id_token {
            Some(token) => identity::verify(client, &self.auth, token, CLIENT_ID, None)
                .await
                .inspect_err(|error| {
                    diagnostic("identity", "verify", None, Some(error.code));
                })?,
            None => match previous {
                Some(Grant::ChatGptOAuth {
                    subject,
                    account_id,
                    email,
                    ..
                }) => identity::Identity {
                    sub: subject.clone(),
                    email: email.clone(),
                    account_id: Some(account_id.clone()),
                },
                _ => {
                    return Err(sailry_protocol::Fault::invalid(
                        "authorization response has no identity token",
                    ));
                }
            },
        };
        tokens.grant(identity, previous).inspect_err(|error| {
            diagnostic("grant", "validate", None, Some(error.code));
        })
    }
}

// Diagnostics are fixed metadata, never authorization payloads or error text.
async fn document<T: DeserializeOwned>(
    request: RequestBuilder,
    stage: &'static str,
) -> Result<T, Fault> {
    let (status, body) = response(request).await.inspect_err(|error| {
        diagnostic(stage, "transport", None, Some(error.code));
    })?;
    if !status.is_success() {
        let error = http_error(status);
        diagnostic(stage, "http_rejected", Some(status), Some(error.code));
        return Err(error);
    }
    let value = decode(&body).inspect_err(|error| {
        diagnostic(stage, "decode", Some(status), Some(error.code));
    })?;
    diagnostic(stage, "http_success", Some(status), None);
    Ok(value)
}

fn diagnostic(
    stage: &'static str,
    reason: &'static str,
    status: Option<StatusCode>,
    code: Option<ErrorCode>,
) {
    use crate::logging::{Area, Entry, Phase};
    let mut entry = Entry::new(
        Area::Node,
        if code.is_some() {
            Phase::Failed
        } else {
            Phase::Completed
        },
    );
    entry.operation = Some("provider_authorization");
    entry.source_kind = Some("chat_gpt_oauth");
    entry.stage = Some(stage);
    entry.reason = Some(reason);
    entry.upstream_status = status.map(|value| u64::from(value.as_u16()));
    entry.code = code;
    if code.is_some() {
        entry.emit();
    } else {
        entry.emit_debug();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Poll {
    Pending,
    SlowDown,
}

fn pending(status: StatusCode, body: &[u8]) -> Option<Poll> {
    if status.is_success() {
        return None;
    }
    if matches!(status, StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) {
        return Some(Poll::Pending);
    }
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    match error
        .as_str()
        .or_else(|| error.get("code").and_then(Value::as_str))?
    {
        "deviceauth_authorization_pending" => Some(Poll::Pending),
        "slow_down" => Some(Poll::SlowDown),
        _ => None,
    }
}

fn interval(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str()?.trim().parse().ok())
        })
        .unwrap_or(5)
        .clamp(1, 900)
}

fn required(value: &Secret) -> Result<(), Fault> {
    if value.expose().is_empty()
        || value.expose().len() > 16384
        || value.expose().chars().any(char::is_control)
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization response is invalid",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
