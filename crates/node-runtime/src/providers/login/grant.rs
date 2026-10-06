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
        account_id: String,
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
    pub(crate) fn renewal_pending(&self) -> bool {
        match self {
            Self::ChatGpt {
                renewal_pending, ..
            } => *renewal_pending,
            Self::Copilot { .. } => false,
        }
    }

    pub(crate) fn begin_renewal(&mut self) {
        if let Self::ChatGpt {
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
                    account_id: first, ..
                },
                Self::ChatGpt {
                    account_id: second, ..
                },
            ) => first == second,
            (Self::Copilot { github: first, .. }, Self::Copilot { github: second, .. }) => {
                first == second
            }
            _ => false,
        }
    }
    pub(crate) fn expires_at_ms(&self) -> u64 {
        match self {
            Self::ChatGpt { expires_at_ms, .. } | Self::Copilot { expires_at_ms, .. } => {
                *expires_at_ms
            }
        }
    }
    pub(crate) fn authentication(&self) -> Authentication {
        match self {
            Self::ChatGpt { .. } => Authentication::ChatGpt,
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
                account_id,
                ..
            } => {
                if account_id.is_empty()
                    || account_id.len() > 256
                    || !account_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    return Err(invalid("authorization account identifier is invalid"));
                }
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
        for token in [access, renewal] {
            if token.expose().is_empty()
                || token.expose().len() > 16384
                || reqwest::header::HeaderValue::from_str(token.expose()).is_err()
            {
                return Err(invalid("authorization token is invalid"));
            }
        }
        if expires == 0 || expires > i64::MAX as u64 {
            return Err(invalid("authorization expiry is invalid"));
        }
        Ok(())
    }
}
