use super::{document, merge};
use sailry_protocol::plugin::mcp::Configuration;
use serde_json::json;

fn configuration() -> Configuration {
    serde_json::from_value(json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers":{
            "read":{"type":"streamable-http","url":"https://example.com/read","headers":{}},
            "write":{"type":"stdio","command":"fixture","args":["write"],"env":{"MODE":"fixture"}}
        }
    }))
    .unwrap()
}

#[test]
fn edits_one_server_without_replacing_siblings() {
    let original = configuration();
    let source = document(&original, Some("read")).unwrap();
    let mut server: serde_json::Value = serde_json::from_str(&source).unwrap();
    assert!(server.get("mcpServers").is_none());
    server["url"] = json!("https://example.com/new");
    let mut refreshed = original.clone();
    let mut sibling = serde_json::to_value(&refreshed.servers["write"]).unwrap();
    sibling["args"] = json!(["latest"]);
    refreshed
        .servers
        .insert("write".into(), serde_json::from_value(sibling).unwrap());
    let edited = merge(Some(&refreshed), Some("read"), &server.to_string()).unwrap();
    assert_eq!(edited.schema, original.schema);
    assert_eq!(edited.servers["write"], refreshed.servers["write"]);
    assert_ne!(edited.servers["write"], original.servers["write"]);
    assert_ne!(edited.servers["read"], original.servers["read"]);
}

#[test]
fn rejects_removed_servers_and_invalid_drafts() {
    let original = configuration();
    let source = document(&original, Some("read")).unwrap();
    assert_eq!(
        merge(Some(&original), Some("missing"), &source).unwrap_err(),
        "plugins_settings_conflict"
    );
    assert_eq!(
        merge(None, Some("read"), &source).unwrap_err(),
        "plugins_settings_conflict"
    );
    assert_eq!(
        merge(Some(&original), Some("read"), "{}").unwrap_err(),
        "mcp_configuration_invalid"
    );
    assert_eq!(original, configuration());
}

#[test]
fn preserves_the_standard_document_editor() {
    let original = configuration();
    let source = document(&original, None).unwrap();
    assert_eq!(merge(Some(&original), None, &source).unwrap(), original);
}
