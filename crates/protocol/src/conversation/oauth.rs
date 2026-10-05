//! Non-secret, provider-specific OAuth request settings.
use super::ModelApi;
use crate::{Authentication, ErrorCode, Fault};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Options {
    ChatGpt {
        catalog_version: String,
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
                // Reviewed Codex catalog baseline, independent of Sailry's version.
                catalog_version: "0.160.0".into(),
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

    /// Wire checks only; the execution Node also validates HTTP values and versions.
    pub fn validate(&self, authentication: Authentication, api: ModelApi) -> Result<(), Fault> {
        let fields: &[&str] = match (self, authentication, api) {
            (
                Self::ChatGpt {
                    catalog_version,
                    user_agent,
                },
                Authentication::ChatGpt,
                ModelApi::Responses,
            ) => &[catalog_version, user_agent],
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
        Ok(())
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
