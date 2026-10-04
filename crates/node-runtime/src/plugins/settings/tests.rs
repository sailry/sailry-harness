use super::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};

fn schema(properties: Value) -> Value {
    json!({
        "$schema": SCHEMA, "type": "object", "additionalProperties": false,
        "properties": properties
    })
}

fn parse(value: Value) -> Result<Schema, Fault> {
    let servers = mcp::parse(&serde_json::to_vec(&json!({
        "$schema": mcp::SCHEMA,
        "mcpServers": {
            "local": {"type":"stdio", "command":"tool", "env":{"TOKEN":"", "token":"", "FIXED":"present"}},
            "remote": {"type":"streamable-http", "url":"https://example.com/mcp", "headers":{"Authorization":""}},
            "legacy": {"type":"sse", "url":"https://example.com/sse", "headers":{"Authorization":""}}
        }
    })).unwrap(), &mut Vec::new()).unwrap();
    validation::parse(&serde_json::to_vec(&value).unwrap(), &servers)
}

#[test]
fn validates_tab_membership_and_enable_fields() {
    let mut document = schema(json!({
        "name": {"type": "string"},
        "enabled": {"type": "boolean", "default": true},
        "model": {"type": "string"}
    }));
    let tabs = json!([
        {"id": "human", "title": "Player 1", "fields": ["name"]},
        {"id": "opponent", "title": "Player 2", "fields": ["enabled", "model"], "enabled": "enabled"}
    ]);
    document["x-sailry-tabs"] = tabs.clone();
    let parsed = parse(document.clone()).unwrap();
    assert_eq!(parsed.tabs.len(), 2);
    assert_eq!(parsed.tabs[1].enabled.as_deref(), Some("enabled"));
    for invalid in [
        json!([tabs[0], tabs[0]]),
        json!([{"id": "one", "title": "Player", "fields": ["missing"]}]),
        json!([{"id": "one", "title": "Player", "fields": ["name", "name"]}]),
        json!([{"id": "one", "title": "Player", "fields": ["name"], "enabled": "name"}]),
        json!([{"id": "one", "title": "Player", "fields": ["name"], "enabled": "enabled"}]),
        json!([{"id": "one", "title": "Player", "fields": []}]),
        json!([{"id": "one", "title": " ", "fields": ["name"]}]),
    ] {
        document["x-sailry-tabs"] = invalid;
        assert!(parse(document.clone()).is_err(), "accepted {document}");
    }
}

#[test]
fn accepts_fields_and_secret_slots() {
    let mut value = schema(json!({
        "model": {"type":"string", "x-sailry-model":true},
        "endpoint": {"type":"string", "title":"Endpoint", "minLength":1, "maxLength":256, "default":"https://example.com"},
        "count": {"type":"integer", "minimum":1, "maximum":4, "enum":[1,2,4], "default":2},
        "ratio": {"type":"number", "minimum":-1.5, "maximum":1.5},
        "enabled": {"type":"boolean", "default":true},
        "token": {"type":"string", "x-sailry-secret":{"server":"local", "env":"TOKEN"}},
        "authorization": {"type":"string", "x-sailry-secret":{"server":"remote", "header":"authorization"}},
        "legacy": {"type":"string", "x-sailry-secret":{"server":"legacy", "header":"authorization"}}
    }));
    value["required"] = json!(["endpoint", "token"]);
    let parsed = parse(value).unwrap();
    assert_eq!(parsed.properties.len(), 8);
    assert!(parsed.properties["model"].model);
    assert_eq!(parsed.properties["count"].default, Some(json!(2)));
    assert_eq!(
        parsed.properties["token"]
            .secret
            .as_ref()
            .unwrap()
            .env
            .as_deref(),
        Some("TOKEN")
    );
    let encoded = serde_json::to_value(&parsed).unwrap();
    assert!(parse(encoded).is_ok());
}

#[test]
fn rejects_unsupported_shapes() {
    for property in [
        json!({"type":"object", "properties":{}}),
        json!({"type":"boolean", "x-sailry-model":true}),
        json!({"type":"string", "x-sailry-model":true, "enum":["fixture"]}),
        json!({"type":"string", "x-sailry-model":true, "x-sailry-secret":{"server":"local", "env":"TOKEN"}}),
        json!({"type":"array"}),
        json!({"type":"string", "$ref":"https://example.com/schema"}),
        json!({"type":"string", "pattern":".*"}),
        json!({"type":"string", "minLength":4, "maxLength":3}),
        json!({"type":"string", "maxLength":MAX_STRING+1}),
        json!({"type":"integer", "minLength":1}),
        json!({"type":"boolean", "minimum":1}),
        json!({"type":"integer", "minimum":5, "maximum":4}),
        json!({"type":"string", "enum":[]}),
        json!({"type":"string", "enum":["a", "a"]}),
        json!({"type":"string", "default":null}),
        json!({"type":"boolean", "default":"true"}),
        json!({"type":"string", "title":null}),
        json!({"type":"string", "enum":vec!["value"; MAX_CHOICES+1]}),
    ] {
        assert!(
            parse(schema(json!({"value":property}))).is_err(),
            "accepted {property}"
        );
    }
    for (key, value) in [
        ("$schema", json!("https://example.com/schema")),
        ("$ref", json!("file:///private/settings.json")),
        ("additionalProperties", json!(true)),
        ("required", json!(["missing"])),
        ("required", json!(["value", "value"])),
        ("title", json!("x".repeat(MAX_STRING + 1))),
    ] {
        let mut document = schema(json!({"value":{"type":"string"}}));
        document[key] = value;
        assert!(parse(document).is_err(), "accepted {key}");
    }
    let properties: BTreeMap<_, _> = (0..MAX_FIELDS + 1)
        .map(|index| (format!("field{index}"), json!({"type":"string"})))
        .collect();
    assert!(parse(schema(json!(properties))).is_err());
    assert!(validation::parse(&vec![b' '; MAX_BYTES + 1], &BTreeMap::new()).is_err());
}

#[test]
fn checks_unicode_and_exact_numbers() {
    assert!(
        parse(schema(
            json!({"value":{"type":"string", "minLength":2, "maxLength":2, "default":"汉🙂"}})
        ))
        .is_ok()
    );
    for property in [
        json!({"type":"string", "maxLength":1, "default":"汉🙂"}),
        json!({"type":"string", "default":"x".repeat(MAX_STRING+1)}),
        json!({"type":"integer", "maximum":9007199254740992u64, "default":9007199254740993u64}),
        json!({"type":"integer", "minimum":9007199254740993u64, "maximum":9007199254740992u64}),
        json!({"type":"number", "minimum":-2, "default":-2.5}),
        json!({"type":"integer", "enum":[1, 2], "default":3}),
    ] {
        assert!(
            parse(schema(json!({"value":property}))).is_err(),
            "accepted {property}"
        );
    }
}

#[test]
fn rejects_ambiguous_or_embedded_secrets() {
    for binding in [
        Value::Null,
        json!({"server":"missing", "env":"TOKEN"}),
        json!({"server":"local", "env":"FIXED"}),
        json!({"server":"local", "env":"ABSENT"}),
        json!({"server":"local", "env":"TOKEN", "header":"Authorization"}),
        json!({"server":"local", "header":"Authorization"}),
        json!({"server":"remote", "env":"TOKEN"}),
        json!({"server":"legacy", "env":"TOKEN"}),
        json!({"server":"remote", "header":"Absent"}),
        json!({"server":"remote", "header":"Authorization", "value":"hidden"}),
    ] {
        assert!(
            parse(schema(
                json!({"token":{"type":"string", "x-sailry-secret":binding}})
            ))
            .is_err()
        );
    }
    for property in [
        json!({"type":"boolean", "x-sailry-secret":{"server":"local", "env":"TOKEN"}}),
        json!({"type":"string", "default":"embedded", "x-sailry-secret":{"server":"local", "env":"TOKEN"}}),
        json!({"type":"string", "enum":["embedded"], "x-sailry-secret":{"server":"local", "env":"TOKEN"}}),
    ] {
        assert!(parse(schema(json!({"token":property}))).is_err());
    }
    for (server, slot, first, second) in [
        ("local", "env", "TOKEN", "token"),
        ("remote", "header", "Authorization", "authorization"),
        ("legacy", "header", "Authorization", "authorization"),
    ] {
        let mut one = json!({"server":server});
        let mut two = one.clone();
        one[slot] = json!(first);
        two[slot] = json!(second);
        assert!(
            parse(schema(json!({
                "first":{"type":"string", "x-sailry-secret":one},
                "second":{"type":"string", "x-sailry-secret":two}
            })))
            .is_err()
        );
    }
}

#[test]
fn unsupported_secret_metadata_remains_unavailable() {
    for binding in [
        json!({"server": "local", "env": "TOKEN", "unused": true}),
        json!({"server": "local", "env": "TOKEN", "unsupported": true}),
        json!({"origin": "https://example.com", "header": "Authorization", "connector": "fixture"}),
        json!({"origin": "https://example.com", "header": "Authorization", "unsupported": false, "unused": {"payload": "private-marker"}}),
    ] {
        let value = schema(json!({"token": {"type": "string", "x-sailry-secret": binding}}));
        let loaded: Schema = serde_json::from_value(value.clone()).unwrap();
        assert!(
            loaded.properties["token"]
                .secret
                .as_ref()
                .unwrap()
                .unsupported
        );
        let error = parse(value).unwrap_err();
        assert_eq!(error.message, "unsupported protected setting binding");
        assert!(!error.message.contains("private-marker"));
    }
}

#[test]
fn rejects_execution_control_slots() {
    for (server, slot, name) in [
        ("local", "env", "SAILRY_PLUGIN_SETTINGS_FILE"),
        ("local", "env", "PLUGIN_ROOT"),
        ("local", "env", "plugin_data"),
        ("remote", "header", "Mcp-Session-Id"),
        ("remote", "header", "MCP-Protocol-Version"),
        ("remote", "header", "Content-Type"),
        ("remote", "header", "Accept"),
    ] {
        let servers = mcp::parse(&serde_json::to_vec(&json!({
            "$schema": mcp::SCHEMA,
            "mcpServers": {
                "local": {"type":"stdio", "command":"tool", "env":{name:""}},
                "remote": {"type":"streamable-http", "url":"https://example.com/mcp", "headers":{name:""}}
            }
        })).unwrap(), &mut Vec::new()).unwrap();
        let binding = json!({"server":server, slot:name});
        let value = schema(json!({"token":{"type":"string", "x-sailry-secret":binding}}));
        assert!(
            validation::parse(&serde_json::to_vec(&value).unwrap(), &servers).is_err(),
            "accepted {name}"
        );
    }
}

#[test]
fn confines_resources_and_isolates_errors() {
    let (_temp, host, package) = crate::plugins::tests::fixture();
    fs::create_dir(package.join("dev.sailry.platform/config")).unwrap();
    fs::write(
        package.join("dev.sailry.platform/config/settings.json"),
        schema(json!({"value":{"type":"string"}})).to_string(),
    )
    .unwrap();
    for (path, valid, extension) in [
        ("./dev.sailry.platform/config/settings.json", true, true),
        ("dev.sailry.platform/config/settings.json", true, true),
        ("dev.sailry.platform/../settings.json", false, true),
        ("dev.sailry.platform/config/missing.json", false, true),
        ("dev.sailry.platform/config/settings.js", false, true),
        ("/dev.sailry.platform/settings.json", false, false),
    ] {
        fs::write(package.join("plugin.json"), json!({
            "$schema":crate::plugins::manifest::SCHEMA, "name":"example",
            "extensions":{"dev.sailry.platform":{"api_version":"v1", "actions":["files.read"], "settings_schema":path}}
        }).to_string()).unwrap();
        let installed = host.install(&package, "", "example").unwrap();
        assert_eq!(
            installed.settings.is_some(),
            valid,
            "unexpected settings for {path}"
        );
        assert_eq!(installed.skills.len(), 1);
        assert_eq!(installed.extension.is_some(), extension);
        if !valid && extension {
            assert!(
                installed
                    .issues
                    .iter()
                    .any(|issue| issue.path == path && issue.kind == IssueKind::InvalidExtensions)
            );
        }
    }
}

#[test]
fn isolates_invalid_mcp_and_symlinks() {
    let (_temp, host, package) = crate::plugins::tests::fixture();
    fs::write(package.join("plugin.json"), json!({
        "$schema":crate::plugins::manifest::SCHEMA, "name":"example",
        "extensions":{"dev.sailry.platform":{"api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"}}
    }).to_string()).unwrap();
    fs::write(package.join("mcp.json"), "{}").unwrap();
    fs::write(
        package.join("dev.sailry.platform/settings.json"),
        schema(json!({"value":{"type":"boolean"}})).to_string(),
    )
    .unwrap();
    let info = host.install(&package, "", "example").unwrap();
    assert!(info.settings.is_some());
    assert!(
        info.issues
            .iter()
            .any(|issue| issue.kind == IssueKind::InvalidMcp)
    );
    #[cfg(unix)]
    {
        fs::remove_file(package.join("dev.sailry.platform/settings.json")).unwrap();
        std::os::unix::fs::symlink(
            package.join("skills/analysis/SKILL.md"),
            package.join("dev.sailry.platform/settings.json"),
        )
        .unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert!(info.settings.is_none());
        assert_eq!(info.skills.len(), 1);
    }
}

#[test]
fn binds_effort_to_model() {
    let fields = json!({"model":{"type":"string","x-sailry-model":true},
        "effort":{"type":"string","x-sailry-model-effort":"model"}});
    assert!(parse(schema(fields.clone())).is_ok());
    for invalid in [
        json!({"type":"string","x-sailry-model-effort":"missing"}),
        json!({"type":"integer","x-sailry-model-effort":"model"}),
        json!({"type":"string","x-sailry-model-effort":"model","enum":["high"]}),
    ] {
        let mut fields = fields.clone();
        fields["effort"] = invalid;
        assert!(parse(schema(fields)).is_err());
    }
}
