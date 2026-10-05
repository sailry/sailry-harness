//! Node-owned device authorization. Wire references: OpenAI Codex ed6dde9f,
//! rig-core 0.42.0; provenance and license notices are recorded in third_party_licenses.
mod chatgpt;
mod copilot;
mod grant;
mod headers;
mod options;
pub(crate) use grant::Grant;
pub(crate) use options::capture;
pub(crate) use options::effective;

use reqwest::{Client, RequestBuilder, StatusCode};
use sailry_link::CancellationToken;
use sailry_protocol::{
    conversation::{ModelApi, Provider, login::State, oauth::Options},
    *,
};
use serde::de::DeserializeOwned;
use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const DEADLINE: Duration = Duration::from_secs(900);
const BODY_LIMIT: usize = 64 * 1024;
pub(crate) const CHATGPT_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex";
pub(crate) const COPILOT_ENDPOINT: &str = "https://api.githubcopilot.com";

#[derive(Clone)]
pub(crate) struct Service {
    auth: String,
    github: String,
    github_api: String,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) model_endpoint: Option<String>,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            auth: "https://auth.openai.com".into(),
            github: "https://github.com".into(),
            github_api: "https://api.github.com".into(),
            #[cfg(any(test, feature = "test-support"))]
            model_endpoint: None,
        }
    }
}

impl Service {
    pub(crate) async fn refresh(
        &self,
        grant: &Grant,
        options: &Options,
        stop: &CancellationToken,
    ) -> Result<Grant, Fault> {
        let operation = async {
            let client = client(options)?;
            let refreshed = match grant {
                Grant::ChatGpt { .. } => self.refresh_chatgpt(&client, grant).await?,
                Grant::Copilot { github, .. } => {
                    self.exchange_copilot(&client, github.clone(), options)
                        .await?
                }
            };
            refreshed.validate(grant.authentication())?;
            if !refreshed.same_account(grant) {
                return Err(invalid("authorization refresh changed its account"));
            }
            if refreshed.expires_at_ms() <= now_ms()? {
                return Err(expired());
            }
            Ok(refreshed)
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            result = operation => result,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(endpoint: &str) -> Self {
        Self {
            auth: endpoint.into(),
            github: endpoint.into(),
            github_api: endpoint.into(),
            model_endpoint: Some(endpoint.into()),
        }
    }

    pub(crate) async fn authorize<P, F>(
        &self,
        authentication: Authentication,
        options: &Options,
        progress: P,
        stop: &CancellationToken,
    ) -> Result<Grant, Fault>
    where
        P: FnOnce(State) -> F,
        F: Future<Output = Result<(), Fault>>,
    {
        let expires_at_ms = now_ms()? + DEADLINE.as_millis() as u64;
        let progress = |mut state| {
            if let State::Pending {
                expires_at_ms: reported,
                ..
            } = &mut state
            {
                *reported = (*reported).min(expires_at_ms);
            }
            progress(state)
        };
        let operation = async {
            let client = client(options)?;
            let grant = match authentication {
                Authentication::ChatGpt => self.chatgpt(&client, progress).await?,
                Authentication::Copilot => self.copilot(&client, options, progress).await?,
                Authentication::ApiKey | Authentication::Host => {
                    return Err(invalid("provider does not use device authorization"));
                }
            };
            grant.validate(authentication)?;
            if grant.expires_at_ms() <= now_ms()? {
                return Err(expired());
            }
            Ok(grant)
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(DEADLINE, operation) => result.map_err(|_| expired())?,
        }
    }
}

pub(crate) fn validate(provider: &Provider) -> Result<(), Fault> {
    let endpoint = provider.endpoint.trim_end_matches('/');
    let valid = match provider.authentication {
        Authentication::ApiKey | Authentication::Host => true,
        Authentication::ChatGpt => {
            provider.api == ModelApi::Responses && endpoint == CHATGPT_ENDPOINT
        }
        Authentication::Copilot => {
            matches!(
                provider.api,
                ModelApi::ChatCompletions | ModelApi::Responses
            ) && endpoint == COPILOT_ENDPOINT
        }
    };
    if !valid {
        return Err(invalid(
            "provider authentication requires its supported API and service endpoint",
        ));
    }
    match &provider.oauth {
        Some(options) => options::validate(provider.authentication, provider.api, options),
        None if matches!(
            provider.authentication,
            Authentication::ChatGpt | Authentication::Copilot
        ) =>
        {
            effective(provider).map(|_| ())
        }
        None => Ok(()),
    }
}

pub(crate) fn transferable(authentication: Authentication) -> Result<(), Fault> {
    if authentication == Authentication::Host {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "select a cloud identity provider on the execution Node",
        ));
    }
    if authentication == Authentication::ChatGpt {
        // Independent Nodes cannot coordinate a rotating token family after copying it.
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "ChatGPT sign-in is required on the execution Node",
        ));
    }
    Ok(())
}

fn client(options: &Options) -> Result<Client, Fault> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .user_agent(options.user_agent())
        .build()
        .map_err(|_| unavailable())
}

async fn response(request: RequestBuilder) -> Result<(StatusCode, Zeroizing<Vec<u8>>), Fault> {
    let mut response = request.send().await.map_err(|_| unavailable())?;
    let status = response.status();
    if status.is_redirection() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "authorization redirect was refused",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > BODY_LIMIT as u64)
    {
        return Err(invalid("authorization response exceeds its size limit"));
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if body.len() + chunk.len() > BODY_LIMIT {
            return Err(invalid("authorization response exceeds its size limit"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}

async fn json<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, Fault> {
    let (status, body) = response(request).await?;
    if !status.is_success() {
        return Err(http_error(status));
    }
    decode(&body)
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, Fault> {
    serde_json::from_slice(body).map_err(|_| invalid("authorization response is invalid"))
}

fn http_error(status: StatusCode) -> Fault {
    if matches!(status.as_u16(), 400 | 401 | 403) {
        Fault::new(
            ErrorCode::PermissionDenied,
            "authorization failed; sign in again",
        )
    } else {
        unavailable()
    }
}

fn prompt(url: &str, user_code: String, duration: Duration) -> Result<State, Fault> {
    if user_code.is_empty()
        || user_code.len() > 64
        || !user_code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid("authorization code is invalid"));
    }
    Ok(State::Pending {
        verification_url: url.into(),
        user_code,
        expires_at_ms: now_ms()? + duration.as_millis() as u64,
    })
}

fn now_ms() -> Result<u64, Fault> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .map_err(|_| Fault::new(ErrorCode::Internal, "system clock is invalid"))
}

fn invalid(message: &'static str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
fn unavailable() -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        "authorization service is unavailable",
    )
}
fn expired() -> Fault {
    Fault::new(ErrorCode::Expired, "authorization expired; sign in again")
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Unavailable, "authorization was cancelled")
}
