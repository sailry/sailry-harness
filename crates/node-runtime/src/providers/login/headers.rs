use super::*;
use reqwest::header::{HeaderMap, HeaderValue};

impl Grant {
    pub(crate) fn access(&self) -> &Secret {
        match self {
            Self::Plugin { record, .. } => &record.issued.access,
            Self::ChatGpt { access, .. }
            | Self::ChatGptOAuth { access, .. }
            | Self::ClaudeOAuth { access, .. }
            | Self::Copilot { access, .. } => access,
        }
    }

    pub(crate) fn endpoint(&self) -> &str {
        match self {
            Self::Plugin { record, .. } => &record.target.endpoint,
            Self::ChatGpt { .. } => CHATGPT_ENDPOINT,
            Self::ChatGptOAuth { .. } => CODEX_ENDPOINT,
            Self::ClaudeOAuth { .. } => CLAUDE_ENDPOINT,
            Self::Copilot { endpoint, .. } => endpoint,
        }
    }

    pub(crate) fn headers(&self, options: &Options) -> Result<HeaderMap, Fault> {
        if let Self::Plugin { record, .. } = self {
            if !matches!(options, Options::Plugin { package, flow, endpoint, .. } if package.name == record.target.package && flow == &record.target.flow && endpoint == &record.target.endpoint)
            {
                return Err(Fault::invalid(
                    "plugin OAuth settings do not match the authorization",
                ));
            }
            return record.issued.headers(&record.target.inject);
        }
        let mut headers = HeaderMap::new();
        let mut access = HeaderValue::from_str(&format!("Bearer {}", self.access().expose()))
            .map_err(|_| sailry_protocol::Fault::invalid("authorization header is invalid"))?;
        access.set_sensitive(true);
        headers.insert("authorization", access);
        headers.insert(
            "user-agent",
            HeaderValue::from_str(options.user_agent())
                .map_err(|_| sailry_protocol::Fault::invalid("OAuth header value is invalid"))?,
        );
        match (self, options) {
            (Self::ClaudeOAuth { .. }, Options::ClaudeOAuth { .. }) => {
                for (name, value) in [
                    ("anthropic-version", "2023-06-01"),
                    ("anthropic-beta", "claude-code-20250219,oauth-2025-04-20"),
                    ("anthropic-dangerous-direct-browser-access", "true"),
                    ("x-app", "cli"),
                ] {
                    headers.insert(name, HeaderValue::from_static(value));
                }
            }
            (Self::ChatGpt { .. }, Options::ChatGpt { .. }) => {
                if !self.plan_enabled() {
                    return Err(Fault::new(
                        ErrorCode::NotConfigured,
                        "ChatGPT plan use is not enabled",
                    ));
                }
            }
            (Self::ChatGptOAuth { account_id, .. }, Options::ChatGptOAuth { .. }) => {
                // Codex backend identity follows reviewed CC Switch 5ae6ad3 and
                // CLIProxyAPI 0f96f56; this is not SIWC's public API contract.
                headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
                headers.insert(
                    "version",
                    HeaderValue::from_str(options.catalog_version().ok_or_else(|| {
                        sailry_protocol::Fault::invalid(
                            "Codex catalog compatibility revision is required",
                        )
                    })?)
                    .map_err(|_| {
                        sailry_protocol::Fault::invalid("OAuth header value is invalid")
                    })?,
                );
                headers.insert(
                    "chatgpt-account-id",
                    HeaderValue::from_str(account_id).map_err(|_| {
                        sailry_protocol::Fault::invalid(
                            "authorization account identifier is invalid",
                        )
                    })?,
                );
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
                    HeaderValue::from_str(editor_version).map_err(|_| {
                        sailry_protocol::Fault::invalid("OAuth header value is invalid")
                    })?,
                );
                headers.insert(
                    "editor-plugin-version",
                    HeaderValue::from_str(editor_plugin_version).map_err(|_| {
                        sailry_protocol::Fault::invalid("OAuth header value is invalid")
                    })?,
                );
                for (name, value) in [
                    ("copilot-integration-id", "vscode-chat"),
                    ("x-github-api-version", "2025-04-01"),
                ] {
                    headers.insert(name, HeaderValue::from_static(value));
                }
            }
            _ => {
                return Err(sailry_protocol::Fault::invalid(
                    "OAuth settings do not match the authorization",
                ));
            }
        }
        Ok(headers)
    }
}
