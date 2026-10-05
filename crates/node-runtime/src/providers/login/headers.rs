use super::*;
use reqwest::header::{HeaderMap, HeaderValue};

impl Grant {
    pub(crate) fn access(&self) -> &Secret {
        match self {
            Self::ChatGpt { access, .. } | Self::Copilot { access, .. } => access,
        }
    }

    pub(crate) fn endpoint(&self) -> &str {
        match self {
            Self::ChatGpt { .. } => CHATGPT_ENDPOINT,
            Self::Copilot { endpoint, .. } => endpoint,
        }
    }

    pub(crate) fn headers(&self, options: &Options) -> Result<HeaderMap, Fault> {
        let mut headers = HeaderMap::new();
        let mut access = HeaderValue::from_str(&format!("Bearer {}", self.access().expose()))
            .map_err(|_| invalid("authorization header is invalid"))?;
        access.set_sensitive(true);
        headers.insert("authorization", access);
        headers.insert(
            "user-agent",
            HeaderValue::from_str(options.user_agent())
                .map_err(|_| invalid("OAuth header value is invalid"))?,
        );
        match (self, options) {
            (Self::ChatGpt { account_id, .. }, Options::ChatGpt { .. }) => {
                let mut account = HeaderValue::from_str(account_id)
                    .map_err(|_| invalid("authorization account is invalid"))?;
                account.set_sensitive(true);
                headers.insert("chatgpt-account-id", account);
                headers.insert("originator", HeaderValue::from_static("sailry"));
            }
            (
                Self::Copilot { .. },
                Options::Copilot {
                    editor_version,
                    editor_plugin_version,
                    ..
                },
            ) => {
                headers.insert(
                    "editor-version",
                    HeaderValue::from_str(editor_version)
                        .map_err(|_| invalid("OAuth header value is invalid"))?,
                );
                headers.insert(
                    "editor-plugin-version",
                    HeaderValue::from_str(editor_plugin_version)
                        .map_err(|_| invalid("OAuth header value is invalid"))?,
                );
                for (name, value) in [
                    ("copilot-integration-id", "vscode-chat"),
                    ("x-github-api-version", "2025-04-01"),
                ] {
                    headers.insert(name, HeaderValue::from_static(value));
                }
            }
            _ => return Err(invalid("OAuth settings do not match the authorization")),
        }
        Ok(headers)
    }
}
