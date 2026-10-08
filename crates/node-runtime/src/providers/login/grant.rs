use super::*;
use serde::{Deserialize, Serialize};

/// Inline provider authorization fields. Public projections never include this representation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Grant {
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
    Copilot {
        access: Secret,
        github: Secret,
        expires_at_ms: u64,
        endpoint: String,
    },
}

impl Grant {
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
        } = self
        {
            *renewal_pending = true;
        }
    }

    pub(crate) fn same_account(&self, other: &Self) -> bool {
        match (self, other) {
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
            _ => false,
        }
    }
    pub(crate) fn expires_at_ms(&self) -> u64 {
        match self {
            Self::ChatGpt { expires_at_ms, .. }
            | Self::ChatGptOAuth { expires_at_ms, .. }
            | Self::Copilot { expires_at_ms, .. } => *expires_at_ms,
        }
    }
    pub(crate) fn authentication(&self) -> Authentication {
        match self {
            Self::ChatGpt { .. } => Authentication::ChatGpt,
            Self::ChatGptOAuth { .. } => Authentication::ChatGptOAuth,
            Self::Copilot { .. } => Authentication::Copilot,
        }
    }

    pub(crate) fn encode(&self) -> Result<Secret, Fault> {
        serde_json::to_string(self)
            .map(Secret::new)
            .map_err(|_| invalid("authorization record is invalid"))
    }

    pub(crate) fn decode(authentication: Authentication, value: &Secret) -> Result<Self, Fault> {
        let grant: Self = decode(value.expose().as_bytes())?;
        grant.validate(authentication)?;
        Ok(grant)
    }

    pub(crate) fn validate(&self, authentication: Authentication) -> Result<(), Fault> {
        if self.authentication() != authentication {
            return Err(invalid(
                "authorization record belongs to another provider type",
            ));
        }
        let (access, renewal, expires) = match self {
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
                    return Err(invalid("authorization account identifier is invalid"));
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
                    return Err(invalid("authorization scopes are invalid"));
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
                    return Err(invalid("authorization account identifier is invalid"));
                }
                token(id_token)?;
                (access, refresh, *expires_at_ms)
            }
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
            return Err(invalid("authorization expiry is invalid"));
        }
        Ok(())
    }
}

fn token(value: &Secret) -> Result<(), Fault> {
    if value.expose().is_empty()
        || value.expose().len() > 16384
        || reqwest::header::HeaderValue::from_str(value.expose()).is_err()
    {
        return Err(invalid("authorization token is invalid"));
    }
    Ok(())
}
