//! Non-secret, provider-specific OAuth request settings.
use super::ModelApi;
use crate::{Authentication, ErrorCode, Fault};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CATALOG_VERSION: &str = "0.160.1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Options {
    ChatGpt {
        /// An unset override uses the supported catalog compatibility revision.
        #[serde(skip_serializing_if = "Option::is_none")]
        catalog_version: Option<String>,
        user_agent: String,
    },
    Copilot {
        user_agent: String,
        editor_version: String,
        editor_plugin_version: String,
    },
}

impl Options {
    pub fn defaults(authentication: Authentication) -> Option<Self> {
        match authentication {
            Authentication::ChatGpt => Some(Self::ChatGpt {
                catalog_version: Some(DEFAULT_CATALOG_VERSION.into()),
                user_agent: "Sailry/0.1".into(),
            }),
            Authentication::Copilot => Some(Self::Copilot {
                user_agent: "Sailry/0.1".into(),
                editor_version: "vscode/1.107.0".into(),
                editor_plugin_version: "copilot-chat/0.35.0".into(),
            }),
            Authentication::ApiKey | Authentication::Host => None,
        }
    }

    /// Wire checks only; the execution Node also validates HTTP values.
    pub fn validate(&self, authentication: Authentication, api: ModelApi) -> Result<(), Fault> {
        let fields: &[&str] = match (self, authentication, api) {
            (Self::ChatGpt { user_agent, .. }, Authentication::ChatGpt, ModelApi::Responses) => {
                &[user_agent]
            }
            (
                Self::Copilot {
                    user_agent,
                    editor_version,
                    editor_plugin_version,
                },
                Authentication::Copilot,
                ModelApi::Responses | ModelApi::ChatCompletions,
            ) => &[user_agent, editor_version, editor_plugin_version],
            _ => return Err(invalid("OAuth settings do not match the provider")),
        };
        if fields.iter().any(|value| {
            value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control)
        }) {
            return Err(invalid("OAuth setting is invalid"));
        }
        if let Some(version) = self.catalog_version() {
            let parts = version.split('.').collect::<Vec<_>>();
            if parts.len() != 3
                || parts.iter().any(|part| {
                    part.is_empty()
                        || part.len() > 8
                        || !part.bytes().all(|byte| byte.is_ascii_digit())
                })
            {
                return Err(invalid("catalog version must use major.minor.patch"));
            }
        }
        Ok(())
    }

    pub fn catalog_version(&self) -> Option<&str> {
        match self {
            Self::ChatGpt {
                catalog_version, ..
            } => Some(
                catalog_version
                    .as_deref()
                    .unwrap_or(DEFAULT_CATALOG_VERSION),
            ),
            Self::Copilot { .. } => None,
        }
    }

    pub fn user_agent(&self) -> &str {
        match self {
            Self::ChatGpt { user_agent, .. } | Self::Copilot { user_agent, .. } => user_agent,
        }
    }
}

fn invalid(message: &'static str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_supported_apis() {
        for (authentication, api) in [
            (Authentication::ChatGpt, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::Responses),
            (Authentication::Copilot, ModelApi::ChatCompletions),
        ] {
            let options = Options::defaults(authentication).unwrap();
            options.validate(authentication, api).unwrap();
            assert_eq!(
                serde_json::from_value::<Options>(serde_json::to_value(&options).unwrap()).unwrap(),
                options
            );
        }
        assert!(Options::defaults(Authentication::ApiKey).is_none());
        assert!(Options::defaults(Authentication::Host).is_none());
    }

    #[test]
    fn validates_catalog_overrides() {
        let mut options = Options::defaults(Authentication::ChatGpt).unwrap();
        assert_eq!(options.catalog_version(), Some(DEFAULT_CATALOG_VERSION));
        let Options::ChatGpt {
            catalog_version, ..
        } = &mut options
        else {
            unreachable!()
        };
        *catalog_version = Some("0.161.0".into());
        options
            .validate(Authentication::ChatGpt, ModelApi::Responses)
            .unwrap();
        assert_eq!(options.catalog_version(), Some("0.161.0"));
        assert_eq!(
            serde_json::from_value::<Options>(serde_json::to_value(&options).unwrap()).unwrap(),
            options
        );
        for version in [
            "",
            "v0.161",
            "0.161",
            "0.161.0\r\nx-header: injected",
            "0.161.0.1",
        ] {
            let Options::ChatGpt {
                catalog_version, ..
            } = &mut options
            else {
                unreachable!()
            };
            *catalog_version = Some(version.into());
            assert!(
                options
                    .validate(Authentication::ChatGpt, ModelApi::Responses)
                    .is_err()
            );
        }
    }

    #[test]
    fn rejects_mismatched_or_control_values() {
        let mut options = Options::defaults(Authentication::ChatGpt).unwrap();
        assert!(
            options
                .validate(Authentication::Copilot, ModelApi::Responses)
                .is_err()
        );
        assert!(
            options
                .validate(Authentication::ChatGpt, ModelApi::ChatCompletions)
                .is_err()
        );
        let Options::ChatGpt { user_agent, .. } = &mut options else {
            unreachable!()
        };
        *user_agent = "Sailry\r\nAuthorization: secret".into();
        assert!(
            options
                .validate(Authentication::ChatGpt, ModelApi::Responses)
                .is_err()
        );
    }
}
