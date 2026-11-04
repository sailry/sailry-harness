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
fn scoped_edits() {
    for (scenario, check) in [
        (
            "edits_one_server_without_replacing_siblings",
            (|_scenario: &str| {
                let original = configuration();
                let source = document(&original, Some("read")).unwrap();
                let mut server: serde_json::Value = serde_json::from_str(&source).unwrap();
                assert!(
                    server.get("mcpServers").is_none(),
                    "edits one server without replacing siblings"
                );
                server["url"] = json!("https://example.com/new");
                let mut refreshed = original.clone();
                let mut sibling = serde_json::to_value(&refreshed.servers["write"]).unwrap();
                sibling["args"] = json!(["latest"]);
                refreshed
                    .servers
                    .insert("write".into(), serde_json::from_value(sibling).unwrap());
                let edited = merge(Some(&refreshed), Some("read"), &server.to_string()).unwrap();
                assert_eq!(
                    edited.schema, original.schema,
                    "edits one server without replacing siblings"
                );
                assert_eq!(
                    edited.servers["write"], refreshed.servers["write"],
                    "edits one server without replacing siblings"
                );
                assert_ne!(
                    edited.servers["write"], original.servers["write"],
                    "edits one server without replacing siblings"
                );
                assert_ne!(
                    edited.servers["read"], original.servers["read"],
                    "edits one server without replacing siblings"
                );
            }) as fn(&str),
        ),
        (
            "rejects_removed_servers_and_invalid_drafts",
            (|_scenario: &str| {
                let original = configuration();
                let source = document(&original, Some("read")).unwrap();
                assert_eq!(
                    merge(Some(&original), Some("missing"), &source).unwrap_err(),
                    "plugins_settings_conflict",
                    "rejects removed servers and invalid drafts"
                );
                assert_eq!(
                    merge(None, Some("read"), &source).unwrap_err(),
                    "plugins_settings_conflict",
                    "rejects removed servers and invalid drafts"
                );
                assert_eq!(
                    merge(Some(&original), Some("read"), "{}").unwrap_err(),
                    "mcp_configuration_invalid",
                    "rejects removed servers and invalid drafts"
                );
                assert_eq!(
                    original,
                    configuration(),
                    "rejects removed servers and invalid drafts"
                );
            }),
        ),
        (
            "preserves_the_standard_document_editor",
            (|_scenario: &str| {
                let original = configuration();
                let source = document(&original, None).unwrap();
                assert_eq!(
                    merge(Some(&original), None, &source).unwrap(),
                    original,
                    "preserves the standard document editor"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
