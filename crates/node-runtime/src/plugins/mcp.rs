//! Agent Plugins 1.0.0 MCP declarations. Loading never starts package code.
use super::{invalid, package};
use cap_std::fs::Dir;
use reqwest::{
    Url,
    header::{HeaderName, HeaderValue},
};
use sailry_protocol::{
    Fault,
    plugin::{Issue, IssueKind},
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_BYTES: usize = 256 * 1024;
pub(crate) const MAX_SERVERS: usize = 16;
pub(super) use sailry_protocol::plugin::mcp::SCHEMA;
pub(crate) use sailry_protocol::plugin::mcp::Server;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    #[serde(rename = "$schema")]
    schema: String,
    #[serde(rename = "mcpServers")]
    servers: BTreeMap<String, serde_json::Value>,
}

pub(crate) fn validate_server(server: &Server) -> Result<(), Fault> {
    match server {
        Server::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let relative = command.strip_prefix("./");
            if command.is_empty()
                || command.contains('\0')
                || relative.is_some_and(|path| path.is_empty())
                || (relative.is_none()
                    && (command == "."
                        || command == ".."
                        || command.contains(['/', '\\'])
                        || command.chars().any(char::is_whitespace)))
                || args.iter().any(|arg| arg.contains('\0'))
                || env.iter().any(|(name, value)| {
                    name.is_empty()
                        || name.contains(['=', '\0'])
                        || reserved(name)
                        || value.contains('\0')
                })
                || cwd.contains('\0')
                || cwd_root(cwd).is_none()
            {
                return Err(invalid("invalid MCP process configuration"));
            }
            // Paths are checked against retained package/data roots before launch.
            Ok(())
        }
        Server::Http { url, headers } | Server::Sse { url, headers } => validate_http(url, headers),
    }
}

fn reserved(name: &str) -> bool {
    #[cfg(windows)]
    {
        name.eq_ignore_ascii_case("PLUGIN_ROOT") || name.eq_ignore_ascii_case("PLUGIN_DATA")
    }
    #[cfg(not(windows))]
    {
        matches!(name, "PLUGIN_ROOT" | "PLUGIN_DATA")
    }
}

/// Whether a CWD is rooted in persistent plugin data, followed by its relative suffix.
pub(crate) fn cwd_root(cwd: &str) -> Option<(bool, &str)> {
    if let Some(path) = cwd.strip_prefix("./") {
        return Some((false, path));
    }
    for (prefix, data) in [("${PLUGIN_ROOT}", false), ("${PLUGIN_DATA}", true)] {
        if cwd == prefix {
            return Some((data, ""));
        }
        if let Some(path) = cwd
            .strip_prefix(prefix)
            .and_then(|path| path.strip_prefix('/'))
        {
            return Some((data, path));
        }
    }
    None
}

pub(crate) fn validate_http(
    endpoint: &str,
    headers: &BTreeMap<String, String>,
) -> Result<(), Fault> {
    let url = Url::parse(endpoint).map_err(|_| invalid("invalid MCP endpoint"))?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|host| host.is_loopback())
    });
    // Reject even empty user-information delimiters, which URL normalization may erase.
    let authority = endpoint
        .split_once("://")
        .map(|(_, tail)| tail.split(['/', '?', '#']).next().unwrap_or(""));
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || (url.scheme() == "http" && !loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || authority.is_some_and(|authority| authority.contains('@'))
    {
        return Err(invalid("invalid MCP endpoint"));
    }
    let mut names = BTreeSet::new();
    for (name, value) in headers {
        let name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid("invalid MCP header"))?;
        HeaderValue::from_str(value).map_err(|_| invalid("invalid MCP header"))?;
        if !names.insert(name.as_str().to_owned()) {
            return Err(invalid("duplicate MCP header"));
        }
    }
    Ok(())
}

pub(crate) fn protocol_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "content-type"
            | "accept"
            | "mcp-session-id"
            | "mcp-protocol-version"
            | "last-event-id"
    )
}

pub(crate) fn validate_configuration(
    declared: &BTreeMap<String, Server>,
    configuration: &sailry_protocol::plugin::mcp::Configuration,
) -> Result<(), Fault> {
    if configuration.schema != SCHEMA
        || configuration.servers.len() != declared.len()
        || serde_json::to_vec(configuration)
            .map_err(|_| invalid("invalid MCP configuration"))?
            .len()
            > MAX_BYTES
    {
        return Err(invalid("invalid MCP configuration document"));
    }
    for (name, server) in &configuration.servers {
        validate_server(server)?;
        let same = declared
            .get(name)
            .is_some_and(|declared| declared.transport() == server.transport());
        if !same {
            return Err(invalid("MCP server declaration changed"));
        }
        if let Server::Http { headers, .. } | Server::Sse { headers, .. } = server
            && headers.keys().any(|name| protocol_header(name))
        {
            return Err(invalid("MCP protocol headers are managed by the transport"));
        }
    }
    Ok(())
}

pub(super) fn parse(
    bytes: &[u8],
    issues: &mut Vec<Issue>,
) -> Result<BTreeMap<String, Server>, Fault> {
    let document: Document =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid mcp.json"))?;
    if document.schema != SCHEMA || document.servers.len() > MAX_SERVERS {
        return Err(invalid("unsupported MCP schema or server limit exceeded"));
    }
    let mut servers = BTreeMap::new();
    for (name, value) in document.servers {
        let server = serde_json::from_value::<Server>(value)
            .ok()
            .filter(|server| name.len() <= 256 && validate_server(server).is_ok());
        match server {
            Some(server) => {
                servers.insert(name, server);
            }
            None => issues.push(Issue {
                path: format!("mcp.json#/mcpServers/{}", pointer(&name)),
                kind: IssueKind::InvalidMcpServer,
            }),
        }
    }
    Ok(servers)
}

pub(super) fn discover(directory: &Dir, issues: &mut Vec<Issue>) -> BTreeMap<String, Server> {
    let result = package::read(directory, "mcp.json", MAX_BYTES)
        .and_then(|bytes| bytes.map(|bytes| parse(&bytes, issues)).transpose());
    match result {
        Ok(Some(servers)) => servers,
        Ok(None) => BTreeMap::new(),
        Err(_) => {
            issues.push(Issue {
                path: "mcp.json".into(),
                kind: IssueKind::InvalidMcp,
            });
            BTreeMap::new()
        }
    }
}

fn pointer(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// Exact, single-pass substitution: replacement text is never expanded again.
pub(crate) fn expand(input: &str, root: &str, data: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(tail) = rest.strip_prefix("${PLUGIN_ROOT}") {
            output.push_str(root);
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("${PLUGIN_DATA}") {
            output.push_str(data);
            rest = tail;
        } else {
            output.push_str("${");
            rest = &rest[2..];
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests;
