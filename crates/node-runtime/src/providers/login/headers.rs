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

    pub(crate) fn headers(&self) -> Result<HeaderMap, Fault> {
        let mut headers = HeaderMap::new();
        let mut access = HeaderValue::from_str(&format!("Bearer {}", self.access().expose()))
            .map_err(|_| invalid("authorization header is invalid"))?;
        access.set_sensitive(true);
        headers.insert("authorization", access);
        headers.insert("user-agent", HeaderValue::from_static("Sailry/0.1"));
        match self {
            Self::ChatGpt { account_id, .. } => {
                let mut account = HeaderValue::from_str(account_id)
                    .map_err(|_| invalid("authorization account is invalid"))?;
                account.set_sensitive(true);
                headers.insert("chatgpt-account-id", account);
                headers.insert("originator", HeaderValue::from_static("sailry"));
            }
            Self::Copilot { .. } => {
                for (name, value) in [
                    ("copilot-integration-id", "vscode-chat"),
                    ("editor-version", "vscode/1.107.0"),
                    ("editor-plugin-version", "copilot-chat/0.35.0"),
                    ("x-github-api-version", "2025-04-01"),
                ] {
                    headers.insert(name, HeaderValue::from_static(value));
                }
            }
        }
        Ok(headers)
    }
}
