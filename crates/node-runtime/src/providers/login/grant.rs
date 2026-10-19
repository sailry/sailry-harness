use super::*;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

/// Inline provider authorization fields. Public projections never include this representation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Grant {
    Plugin {
        record: Box<plugin::Record>,
        #[serde(default)]
        renewal_pending: bool,
        #[serde(default)]
        renewal_rejected: bool,
    },
    ChatGpt {
        access: Secret,
        refresh: Secret,
        expires_at_ms: u64,
        earliest_refresh_at: Option<u64>,
        client_id: String,
        subject: String,
        email: Option<String>,
        id_token: Secret,
        scopes: Vec<String>,
        renewal_pending: bool,
    },
    #[serde(rename = "chat_gpt_oauth")]
    ChatGptOAuth {
        access: Secret,
        refresh: Secret,
        expires_at_ms: u64,
        subject: String,
        account_id: String,
        email: Option<String>,
        id_token: Secret,
        renewal_pending: bool,
    },
    #[serde(rename = "claude_oauth")]
    ClaudeOAuth {
        access: Secret,
        refresh: Secret,
        expires_at_ms: u64,
        #[serde(default)]
        renewal_pending: bool,
    },
    Copilot {
        access: Secret,
        github: Secret,
        expires_at_ms: u64,
        endpoint: String,
    },
}

impl Grant {
    pub(crate) fn secrets(&self, headers: &HeaderMap) -> Vec<Secret> {
        // Only exact endpoint metadata already exposed to this captured model can
        // be public. Access, refresh and private client fields remain protected even
        // when their bytes happen to equal the same host or origin.
        let public = match self {
            Grant::Plugin { record, .. } => crate::providers::endpoint_url(&record.target.endpoint)
                .ok()
                .map(|endpoint| {
                    let origin = endpoint.origin().ascii_serialization();
                    record
                        .issued
                        .extra
                        .values()
                        .filter(|value| {
                            endpoint.host_str() == Some(value.expose()) || value.expose() == origin
                        })
                        .map(|value| value.expose())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut values: Vec<_> = headers
            .iter()
            .filter(|(name, value)| value.is_sensitive() || name.as_str() == "chatgpt-account-id")
            .filter_map(|(_, value)| value.to_str().ok())
            .filter(|value| !public.contains(value))
            .map(|value| sailry_protocol::Secret::new(value.to_owned()))
            .collect();
        values.push(self.access().clone());
        // A generation error can echo any private field of its freshly resolved grant,
        // including a rotated refresh token that was not sent as an HTTP header.
        match self {
            Grant::Plugin { record, .. } => {
                values.extend(record.issued.refresh.iter().cloned());
                values.extend(
                    record
                        .issued
                        .extra
                        .values()
                        .filter(|value| !public.contains(&value.expose()))
                        .cloned(),
                );
                values.extend(record.target.fields.values().cloned());
            }
            Grant::ChatGpt {
                refresh, id_token, ..
            }
            | Grant::ChatGptOAuth {
                refresh, id_token, ..
            } => {
                values.extend([refresh.clone(), id_token.clone()]);
            }
            Grant::ClaudeOAuth { refresh, .. } => values.push(refresh.clone()),
            Grant::Copilot { github, .. } => values.push(github.clone()),
        }
        values
    }

    pub(crate) fn plan_enabled(&self) -> bool {
        matches!(self, Self::ChatGpt { scopes, .. }
            if ["resource.invoke", "chatgpt.tokens.use.direct"].iter()
                .all(|required| scopes.iter().any(|scope| scope == required)))
    }

    pub(crate) fn renewal_pending(&self) -> bool {
        match self {
            Self::ChatGpt {
                renewal_pending, ..
            }
            | Self::ChatGptOAuth {
                renewal_pending, ..
            }
            | Self::ClaudeOAuth {
                renewal_pending, ..
            } => *renewal_pending,
            Self::Plugin {
                renewal_pending, ..
            } => *renewal_pending,
            Self::Copilot { .. } => false,
        }
    }

    pub(crate) fn begin_renewal(&mut self) {
        if let Self::ChatGpt {
            renewal_pending, ..
        }
        | Self::ChatGptOAuth {
            renewal_pending, ..
        }
        | Self::ClaudeOAuth {
            renewal_pending, ..
        }
        | Self::Plugin {
            renewal_pending, ..
        } = self
        {
            *renewal_pending = true;
        }
    }

    pub(crate) fn same_account(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Plugin { record: first, .. }, Self::Plugin { record: second, .. }) => {
                first.target == second.target
            }
            (
                Self::ChatGpt {
                    client_id: first_client,
                    subject: first,
                    ..
                },
                Self::ChatGpt {
                    client_id: second_client,
                    subject: second,
                    ..
                },
            ) => first_client == second_client && first == second,
            (
                Self::ChatGptOAuth {
                    subject: first_subject,
                    account_id: first_account,
                    ..
                },
                Self::ChatGptOAuth {
                    subject: second_subject,
                    account_id: second_account,
                    ..
                },
            ) => first_subject == second_subject && first_account == second_account,
            (Self::Copilot { github: first, .. }, Self::Copilot { github: second, .. }) => {
                first == second
            }
            (Self::ClaudeOAuth { .. }, Self::ClaudeOAuth { .. }) => true,
            _ => false,
        }
    }
    pub(crate) fn expires_at_ms(&self) -> u64 {
        match self {
            Self::Plugin { record, .. } => record.issued.expires_at_ms.unwrap_or(i64::MAX as u64),
            Self::ChatGpt { expires_at_ms, .. }
            | Self::ChatGptOAuth { expires_at_ms, .. }
            | Self::ClaudeOAuth { expires_at_ms, .. }
            | Self::Copilot { expires_at_ms, .. } => *expires_at_ms,
        }
    }
    pub(crate) fn authentication(&self) -> Authentication {
        match self {
            Self::Plugin { .. } => Authentication::Plugin,
            Self::ChatGpt { .. } => Authentication::ChatGpt,
            Self::ChatGptOAuth { .. } => Authentication::ChatGptOAuth,
            Self::ClaudeOAuth { .. } => Authentication::ClaudeOAuth,
            Self::Copilot { .. } => Authentication::Copilot,
        }
    }

    pub(crate) fn encode(&self) -> Result<Secret, Fault> {
        serde_json::to_string(self)
            .map(Secret::new)
            .map_err(|_| sailry_protocol::Fault::invalid("authorization record is invalid"))
    }

    pub(crate) fn decode(authentication: Authentication, value: &Secret) -> Result<Self, Fault> {
        let grant: Self = decode(value.expose().as_bytes())?;
        grant.validate(authentication)?;
        Ok(grant)
    }

    pub(crate) fn validate(&self, authentication: Authentication) -> Result<(), Fault> {
        if self.authentication() != authentication {
            return Err(sailry_protocol::Fault::invalid(
                "authorization record belongs to another provider type",
            ));
        }
        if let Self::Plugin { record, .. } = self {
            return record.validate();
        }
        let (access, renewal, expires) = match self {
            Self::Plugin { .. } => unreachable!("plugin grants are validated separately"),
            Self::ChatGpt {
                access,
                refresh,
                expires_at_ms,
                client_id,
                subject,
                email,
                id_token,
                scopes,
                ..
            } => {
                chatgpt::flow::issued_client(client_id)?;
                if subject.is_empty()
                    || subject.len() > 2048
                    || subject.chars().any(char::is_control)
                    || email.as_ref().is_some_and(|value| {
                        value.is_empty()
                            || value.len() > 1024
                            || value.chars().any(char::is_control)
                    })
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "authorization account identifier is invalid",
                    ));
                }
                token(id_token)?;
                if scopes.len() > 128
                    || scopes.iter().any(|scope| {
                        scope.is_empty()
                            || scope.len() > 256
                            || !scope.bytes().all(|byte| {
                                byte == 0x21
                                    || (0x23..=0x5b).contains(&byte)
                                    || (0x5d..=0x7e).contains(&byte)
                            })
                    })
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "authorization scopes are invalid",
                    ));
                }
                (access, refresh, *expires_at_ms)
            }
            Self::ChatGptOAuth {
                access,
                refresh,
                expires_at_ms,
                subject,
                account_id,
                email,
                id_token,
                ..
            } => {
                if [subject, account_id].iter().any(|value| {
                    value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control)
                }) || email.as_ref().is_some_and(|value| {
                    value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
                }) || reqwest::header::HeaderValue::from_str(account_id).is_err()
                {
                    return Err(sailry_protocol::Fault::invalid(
                        "authorization account identifier is invalid",
                    ));
                }
                token(id_token)?;
                (access, refresh, *expires_at_ms)
            }
            Self::ClaudeOAuth {
                access,
                refresh,
                expires_at_ms,
                ..
            } => (access, refresh, *expires_at_ms),
            Self::Copilot {
                access,
                github,
                expires_at_ms,
                endpoint,
                ..
            } => {
                copilot::endpoint(endpoint)?;
                (access, github, *expires_at_ms)
            }
        };
        for value in [access, renewal] {
            token(value)?;
        }
        if expires == 0 || expires > i64::MAX as u64 {
            return Err(sailry_protocol::Fault::invalid(
                "authorization expiry is invalid",
            ));
        }
        Ok(())
    }
}

fn token(value: &Secret) -> Result<(), Fault> {
    if value.expose().is_empty()
        || value.expose().len() > 16384
        || reqwest::header::HeaderValue::from_str(value.expose()).is_err()
    {
        return Err(sailry_protocol::Fault::invalid(
            "authorization token is invalid",
        ));
    }
    Ok(())
}
