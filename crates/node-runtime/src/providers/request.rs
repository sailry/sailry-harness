//! Generation authorization policy shared by GDK and direct provider requests.
use super::login::{self, Grant};
use crate::store::Ingress;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Authentication, CredentialRef, ErrorCode, Fault, RequestId, Secret, SessionId,
    conversation::{ModelApi, Provider, oauth::Options},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(test)]
pub(crate) mod fixture;
mod json;
#[cfg(test)]
mod tests;

pub(crate) enum Credentials {
    ApiKey(Option<Secret>),
    Grant { grant: Box<Grant>, options: Options },
}

pub(crate) struct Resolved {
    pub provider: Provider,
    pub revision: Option<u64>,
    pub credentials: Credentials,
}

/// Resolve through the existing Node credential and refresh owners.
pub(crate) async fn current(
    ingress: &Ingress,
    provider: &Provider,
    reference: Option<&CredentialRef>,
    stop: &CancellationToken,
) -> Result<Resolved, Fault> {
    if provider.authentication == Authentication::ApiKey {
        let credential = match reference {
            Some(reference) => Some(
                ingress
                    .model_credential(reference.clone(), provider.id)
                    .await?,
            ),
            None => None,
        };
        return Ok(Resolved {
            provider: provider.clone(),
            revision: credential
                .as_ref()
                .map(|(credential, _)| credential.revision),
            credentials: Credentials::ApiKey(credential.map(|(_, secret)| secret)),
        });
    }
    let reference = reference
        .cloned()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "provider sign-in is required"))?;
    let options = login::effective(provider)?;
    let authorized = ingress
        .authorization(
            reference,
            provider.id,
            provider.authentication,
            &options,
            stop,
        )
        .await?;
    let mut effective = provider.clone();
    effective.endpoint = authorized.grant.endpoint().into();
    #[cfg(any(test, feature = "test-support"))]
    if let Some(endpoint) = ingress.authorization_endpoint.lock().unwrap().as_ref() {
        effective.endpoint = endpoint.clone();
    }
    Ok(Resolved {
        provider: effective,
        revision: Some(authorized.credential.revision),
        credentials: Credentials::Grant {
            grant: Box::new(authorized.grant),
            options,
        },
    })
}

pub(crate) struct Metadata<'a> {
    pub model: &'a str,
    pub session: SessionId,
    pub request: RequestId,
    pub system: &'a str,
    pub tool_names: &'a [&'a str],
    pub delegated: bool,
    pub user: bool,
    pub vision: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Native,
    Instructions,
}

pub(crate) struct System {
    pub text: String,
    pub placement: Placement,
}

#[derive(Default)]
pub(crate) struct Names {
    pub forward: BTreeMap<String, String>,
    pub reverse: BTreeMap<String, String>,
}

impl Names {
    pub(crate) fn new(names: &[&str]) -> Result<Self, Fault> {
        let mut result = Self::default();
        for name in names {
            let wire = canonical(name);
            if result.reverse.insert(wire.into(), (*name).into()).is_some() {
                return Err(Fault::invalid("authorized model tool names conflict"));
            }
            result.forward.insert((*name).into(), wire.into());
        }
        Ok(result)
    }
}

pub(crate) struct Prepared {
    /// Effective base URL; the selected protocol appends its generation route.
    pub endpoint: String,
    pub headers: HeaderMap,
    pub cached_headers: HeaderMap,
    pub system: System,
    pub parameters: BTreeMap<String, Value>,
    pub remove_params: &'static [&'static str],
    pub names: Names,
    pub secrets: Vec<Secret>,
    api: ModelApi,
    authentication: Authentication,
}

impl Prepared {
    pub(crate) fn apply_json(&self, body: &mut Value) -> Result<(), Fault> {
        json::apply(self, body)
    }
}

pub(crate) fn headers(provider: &Provider, credentials: &Credentials) -> Result<HeaderMap, Fault> {
    match credentials {
        Credentials::Grant { grant, options } => grant.headers(options),
        Credentials::ApiKey(key) => {
            let mut headers = HeaderMap::new();
            let auth = auth(provider, key.as_ref().map_or("", Secret::expose));
            // Google's dedicated GDK provider owns this header separately.
            let auth = match (provider.api, auth) {
                (ModelApi::Gemini, KeyAuth::Bearer(value)) => KeyAuth::Header {
                    name: "x-goog-api-key".into(),
                    value,
                },
                (_, auth) => auth,
            };
            match auth {
                KeyAuth::None => {}
                KeyAuth::Bearer(value) => {
                    insert_secret(&mut headers, "authorization", &format!("Bearer {value}"))?;
                }
                KeyAuth::Header { name, value } => insert_secret(&mut headers, &name, &value)?,
            }
            if matches!(provider.api, ModelApi::Anthropic | ModelApi::AzureAi) {
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
            Ok(headers)
        }
    }
}

pub(crate) fn prepare(
    provider: &Provider,
    credentials: &Credentials,
    metadata: Metadata<'_>,
) -> Result<Prepared, Fault> {
    let cached_headers = headers(provider, credentials)?;
    let secrets = match credentials {
        Credentials::Grant { grant, .. } => grant.secrets(&cached_headers),
        Credentials::ApiKey(key) => key.iter().cloned().collect(),
    };
    let mut prepared = Prepared {
        endpoint: provider.endpoint.clone(),
        headers: cached_headers.clone(),
        cached_headers,
        system: System {
            text: metadata.system.into(),
            placement: Placement::Native,
        },
        parameters: BTreeMap::new(),
        remove_params: &[],
        names: Names::default(),
        secrets,
        api: provider.api,
        authentication: provider.authentication,
    };
    let Credentials::Grant { grant, .. } = credentials else {
        return Ok(prepared);
    };
    match grant.as_ref() {
        Grant::ChatGptOAuth { .. } => {
            prepared.remove_params = &["max_tokens", "temperature"];
            prepared.system.placement = Placement::Instructions;
            prepared
                .parameters
                .insert("instructions".into(), json!(metadata.system));
            prepared.parameters.insert(
                "prompt_cache_key".into(),
                json!(metadata.session.to_string()),
            );
            prepared
                .headers
                .insert("session-id", header(&metadata.session.to_string())?);
            prepared.headers.insert(
                "x-codex-routing-hint",
                header(&format!("model={}", metadata.model))?,
            );
        }
        Grant::ChatGpt { .. } => prepared.remove_params = &["max_tokens", "temperature"],
        Grant::ClaudeOAuth { .. } => {
            // Pi's reviewed subscription identity is an authorization requirement.
            // Source: pi 4270522473e677ddba7b093caa0064a3df51d459 (MIT).
            prepared.system.text = format!(
                "You are Claude Code, Anthropic's official CLI for Claude.\n\n{}",
                metadata.system
            );
            prepared.names = Names::new(metadata.tool_names)?;
        }
        Grant::Copilot { .. } => {
            prepared.headers.insert(
                "openai-intent",
                HeaderValue::from_static("conversation-agent"),
            );
            prepared.headers.insert(
                "x-initiator",
                HeaderValue::from_static(if metadata.user && !metadata.delegated {
                    "user"
                } else {
                    "agent"
                }),
            );
            prepared
                .headers
                .insert("x-request-id", header(&metadata.request.to_string())?);
            if metadata.vision {
                prepared
                    .headers
                    .insert("copilot-vision-request", HeaderValue::from_static("true"));
            }
        }
        Grant::Plugin { .. } => {}
    }
    Ok(prepared)
}

pub(crate) enum KeyAuth {
    None,
    Bearer(String),
    Header { name: String, value: String },
}

/// Preserve GDK client key injection without depending on its HTTP client types.
pub(crate) fn auth(provider: &Provider, key: &str) -> KeyAuth {
    if provider.authentication != Authentication::ApiKey || key.is_empty() {
        return KeyAuth::None;
    }
    if let Some(injection) = provider
        .binding
        .as_ref()
        .and_then(|binding| binding.key_injection.as_ref())
    {
        return KeyAuth::Header {
            name: injection.header.clone(),
            value: injection.template.replace("{key}", key),
        };
    }
    match provider.api {
        ModelApi::Anthropic => KeyAuth::Header {
            name: "x-api-key".into(),
            value: key.into(),
        },
        ModelApi::AzureOpenAi | ModelApi::AzureAi => KeyAuth::Header {
            name: "api-key".into(),
            value: key.into(),
        },
        _ => KeyAuth::Bearer(key.into()),
    }
}

fn header(value: &str) -> Result<HeaderValue, Fault> {
    HeaderValue::from_str(value)
        .map_err(|_| Fault::invalid("model authorization header is invalid"))
}

fn insert_secret(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), Fault> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| Fault::invalid("model authorization header is invalid"))?;
    let mut value = header(value)?;
    value.set_sensitive(true);
    headers.insert(name, value);
    Ok(())
}

pub(crate) fn canonical(name: &str) -> &str {
    const TOOLS: &[&str] = &[
        "Read",
        "Write",
        "Edit",
        "Bash",
        "Grep",
        "Glob",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
        "KillShell",
        "NotebookEdit",
        "Skill",
        "Task",
        "TaskOutput",
        "TodoWrite",
        "WebFetch",
        "WebSearch",
    ];
    TOOLS
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
        .unwrap_or(name)
}
