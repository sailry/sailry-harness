use super::*;
use serde_json::{Value, json};

fn parsed(servers: Value) -> (BTreeMap<String, Server>, Vec<Issue>) {
    let mut issues = Vec::new();
    let result = parse(
        json!({"$schema": SCHEMA, "mcpServers": servers})
            .to_string()
            .as_bytes(),
        &mut issues,
    )
    .unwrap();
    (result, issues)
}

#[test]
fn rejects_invalid_documents() {
    for document in [
        json!({}),
        json!([]),
        json!({"$schema": SCHEMA}),
        json!({"$schema": "other", "mcpServers": {}}),
        json!({"$schema": SCHEMA, "mcpServers": null}),
        json!({"$schema": SCHEMA, "mcpServers": {}, "unknown": true}),
    ] {
        assert!(parse(document.to_string().as_bytes(), &mut Vec::new()).is_err());
    }
    assert!(parsed(json!({})).0.is_empty());
}

#[test]
fn isolates_invalid_servers() {
    let (servers, issues) = parsed(json!({
        "stdio": {"type": "stdio", "command": "node", "args": ["${PLUGIN_ROOT}/server.js"]},
        "http": {"type": "streamable-http", "url": "https://example.test/mcp"},
        "legacy": {"type": "sse", "url": "http://localhost/sse"},
        "a": {"type": "stdio", "command": "node --flag"},
        "b": {"type": "stdio", "command": "/bin/node"},
        "c": {"type": "stdio", "command": "node", "url": "https://example.test"},
        "d": {"type": "stdio", "command": "node", "env": {"PLUGIN_ROOT": "wrong"}},
        "e": {"type": "stdio", "command": "node", "args": null},
        "f": {"type": "streamable-http", "url": "https://example.test", "cwd": "./"},
        "g": {"type": "unknown"},
        "h": {"type": "stdio", "command": "node", "cwd": "/tmp"},
        "i": {"type": "stdio", "command": "node", "env": {"A=B": "value"}},
        "j": {"type": "stdio", "command": "node", "args": ["bad\u{0000}arg"]}
    }));
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["http", "legacy", "stdio"]
    );
    assert_eq!(issues.len(), 10);
    assert!(
        issues
            .iter()
            .all(|issue| issue.kind == IssueKind::InvalidMcpServer)
    );
}

#[test]
fn validates_literal_endpoints() {
    for url in [
        "https://example.test/mcp",
        "http://localhost/mcp",
        "http://127.1.2.3/mcp",
        "http://[::1]/mcp",
    ] {
        assert!(validate_http(url, &BTreeMap::new()).is_ok(), "{url}");
    }
    for url in [
        "http://localhost.example.test",
        "http://192.0.2.1",
        "http://[::2]",
        "https://user@example.test",
        "https://@example.test",
        "https://example.test/#",
        "file:///tmp/mcp",
        "/mcp",
    ] {
        assert!(validate_http(url, &BTreeMap::new()).is_err(), "{url}");
    }
    for headers in [
        BTreeMap::from([
            ("X-Test".into(), "one".into()),
            ("x-test".into(), "two".into()),
        ]),
        BTreeMap::from([("Invalid Name".into(), "value".into())]),
        BTreeMap::from([("X-Test".into(), "bad\r\nvalue".into())]),
    ] {
        assert!(validate_http("https://example.test", &headers).is_err());
    }
    let (servers, _) = parsed(
        json!({"service": {"type": "streamable-http", "url": "https://example.test/${PLUGIN_ROOT}", "headers": {"X-Test": "${PLUGIN_DATA}"}}}),
    );
    let Server::Http { url, headers } = &servers["service"] else {
        panic!("HTTP expected")
    };
    assert!(url.ends_with("${PLUGIN_ROOT}"));
    assert_eq!(headers["X-Test"], "${PLUGIN_DATA}");
}

#[test]
fn expands_exact_placeholders_once() {
    assert_eq!(
        expand(
            "${PLUGIN_ROOT}:${PLUGIN_DATA}:${HOME}:${PLUGIN_ROOT}",
            "/root/${PLUGIN_DATA}",
            "/data"
        ),
        "/root/${PLUGIN_DATA}:/data:${HOME}:/root/${PLUGIN_DATA}"
    );
    assert_eq!(
        expand("🙂${PLUGIN_ROOT}/${unknown}/end", "目录", "data"),
        "🙂目录/${unknown}/end"
    );
}

#[test]
fn accepts_standard_configuration_edits() {
    let (declared, _) = parsed(json!({
        "process":{"type":"stdio","command":"node","args":["server.js"]},
        "service":{"type":"streamable-http","url":"https://example.test/mcp"}
    }));
    let configuration: sailry_protocol::plugin::mcp::Configuration = serde_json::from_value(json!({
        "$schema":SCHEMA,"mcpServers":{
            "process":{"type":"stdio","command":"python","args":["${PLUGIN_ROOT}/server.py"],"cwd":"${PLUGIN_DATA}/work","env":{"TOKEN":"plain-token"}},
            "service":{"type":"streamable-http","url":"http://127.0.0.1:43199/mcp","headers":{"Authorization":"Bearer plain-token","X-Context7-API-Key":"plain-key"}}
        }
    })).unwrap();
    assert!(validate_configuration(&declared, &configuration).is_ok());
    assert!(!format!("{configuration:?}").contains("plain-token"));
    let mut invalid = configuration.clone();
    invalid
        .servers
        .insert("other".into(), configuration.servers["service"].clone());
    assert!(validate_configuration(&declared, &invalid).is_err());
    let mut invalid = configuration.clone();
    invalid.servers.insert(
        "service".into(),
        Server::Sse {
            url: "https://example.test/mcp".into(),
            headers: BTreeMap::new(),
        },
    );
    assert!(validate_configuration(&declared, &invalid).is_err());
    let mut invalid = configuration;
    let Server::Http { headers, .. } = invalid.servers.get_mut("service").unwrap() else {
        unreachable!()
    };
    headers.insert("Mcp-Session-Id".into(), "not-controller-owned".into());
    assert!(validate_configuration(&declared, &invalid).is_err());
}
