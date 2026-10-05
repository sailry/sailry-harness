use super::*;
use serde::Deserialize;

// Public OAuth application identifier used by the existing Copilot integration.
const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Deserialize)]
struct Device {
    device_code: Secret,
    user_code: String,
    verification_uri: String,
    interval: Option<u64>,
    expires_in: Option<u64>,
}
#[derive(Deserialize)]
struct Token {
    access_token: Option<Secret>,
    error: Option<String>,
}
#[derive(Deserialize)]
struct Access {
    token: Secret,
    expires_at: u64,
    endpoints: Option<Endpoints>,
}
#[derive(Deserialize)]
struct Endpoints {
    api: Option<String>,
}

impl Service {
    pub(super) async fn copilot<P, F>(
        &self,
        client: &Client,
        options: &Options,
        progress: P,
    ) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let device: Device = json(
            client
                .post(format!("{}/login/device/code", self.github))
                .header("accept", "application/json")
                .form(&[("client_id", CLIENT_ID), ("scope", "read:user")]),
        )
        .await?;
        if device.verification_uri != "https://github.com/login/device" {
            return Err(invalid("authorization verification URL is invalid"));
        }
        let duration = Duration::from_secs(device.expires_in.unwrap_or(900).clamp(1, 900));
        let deadline = tokio::time::Instant::now() + duration;
        let mut interval = device.interval.unwrap_or(5).clamp(1, 900);
        progress(prompt(
            &device.verification_uri,
            device.user_code,
            duration,
        )?)
        .await?;
        tokio::time::timeout_at(deadline, async {
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                let token: Token = json(
                    client
                        .post(format!("{}/login/oauth/access_token", self.github))
                        .header("accept", "application/json")
                        .form(&[
                            ("client_id", CLIENT_ID),
                            ("device_code", device.device_code.expose()),
                            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ]),
                )
                .await?;
                if let Some(access) = token.access_token {
                    return self.exchange_copilot(client, access, options).await;
                }
                match token.error.as_deref() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => interval = (interval + 5).min(900),
                    Some("expired_token") => return Err(expired()),
                    Some("access_denied") => {
                        return Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "authorization was denied",
                        ));
                    }
                    _ => return Err(unavailable()),
                }
            }
        })
        .await
        .map_err(|_| expired())?
    }

    pub(super) async fn exchange_copilot(
        &self,
        client: &Client,
        github: Secret,
        options: &Options,
    ) -> Result<Grant, Fault> {
        let Options::Copilot {
            editor_version,
            editor_plugin_version,
            ..
        } = options
        else {
            return Err(invalid("OAuth settings do not match the authorization"));
        };
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("token {}", github.expose()))
                .map_err(|_| invalid("authorization token is invalid"))?;
        authorization.set_sensitive(true);
        let access: Access = json(
            client
                .get(format!("{}/copilot_internal/v2/token", self.github_api))
                .header("accept", "application/json")
                .header("authorization", authorization)
                .header("editor-version", editor_version)
                .header("editor-plugin-version", editor_plugin_version),
        )
        .await?;
        let target = access
            .endpoints
            .and_then(|endpoints| endpoints.api)
            .unwrap_or_else(|| COPILOT_ENDPOINT.into());
        endpoint(&target)?;
        Ok(Grant::Copilot {
            access: access.token,
            github,
            expires_at_ms: access
                .expires_at
                .checked_mul(1000)
                .ok_or_else(|| invalid("authorization expiry is invalid"))?,
            endpoint: target,
        })
    }
}

pub(super) fn endpoint(value: &str) -> Result<(), Fault> {
    let url =
        reqwest::Url::parse(value).map_err(|_| invalid("Copilot service endpoint is invalid"))?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || !url.host_str().is_some_and(|host| {
            host == "api.githubcopilot.com" || host.ends_with(".githubcopilot.com")
        })
    {
        return Err(invalid("Copilot service endpoint is invalid"));
    }
    Ok(())
}
