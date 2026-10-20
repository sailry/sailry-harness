//! Manual MCP sources retain their explicit user-selected target permissions.
use super::*;
use sailry_protocol::plugin::connector::{Accounts, Client};

pub(in crate::store) fn declarations(info: &Info) -> Vec<Declaration> {
    let mut declarations = info
        .extension
        .as_ref()
        .map(|extension| extension.connectors.clone())
        .unwrap_or_default();
    if let Some(
        plugin::mcp::Definition::StreamableHttp { url, .. }
        | plugin::mcp::Definition::Sse { url, .. },
    ) = &info.mcp_source
        && let Ok(url) = url::Url::parse(url)
    {
        declarations.push(Declaration {
            id: "server".into(),
            display: plugin::desktop::Navigation {
                label: info.summary.name.clone(),
                locales: Default::default(),
                icon: None,
            },
            accounts: Accounts::Single,
            origins: vec![url.origin().ascii_serialization()],
            auth: vec![Auth::McpOauth {
                server: "server".into(),
                client: None::<Client>,
            }],
            fields: Vec::new(),
            inject: vec![Injection {
                server: Some("server".into()),
                header: Some("Authorization".into()),
                prefix: "Bearer ".into(),
                origin: None,
                env: None,
                field: None,
            }],
            identity: None,
            health: None,
        });
    }
    declarations
}

pub(super) fn endpoint(info: &Info, declaration: &Declaration, url: &str) -> bool {
    declaration.endpoint(url)
        || info.mcp_source.is_some()
            && crate::plugins::mcp::validate_http(url, &Default::default()).is_ok()
            && url::Url::parse(url).is_ok_and(|url| {
                declaration
                    .origins
                    .contains(&url.origin().ascii_serialization())
            })
}
