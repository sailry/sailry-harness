//! Authoring schemas follow the current Rust deserialization contract.
use schemars::generate::SchemaSettings;
use serde_json::{Value, json};

pub(super) fn resource_path(schema: &mut schemars::Schema) {
    schema.insert("pattern".into(), json!(r"^(\./)?dev\.sailry\.platform/"));
}

pub(super) fn resource_paths(schema: &mut schemars::Schema) {
    schema.ensure_object()["items"]["pattern"] = json!(r"^(\./)?dev\.sailry\.platform/");
}

pub const EXTENSION_ID: &str = "urn:sailry:schema:extension:v1";
pub const MANIFEST_ID: &str = "urn:sailry:schema:plugin:v1";
pub const AGENT_PLUGINS_ID: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";

pub fn extension() -> Value {
    let schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<super::Extension>();
    let mut schema = serde_json::to_value(schema).expect("plugin schema must be JSON");
    schema["$id"] = EXTENSION_ID.into();
    schema["title"] = "Sailry Extension".into();
    schema
}

/// The extension remains optional; ordinary Agent Plugins use the same format.
pub fn manifest() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": MANIFEST_ID,
        "title": "Sailry Plugin",
        "allOf": [
            {"$ref": AGENT_PLUGINS_ID},
            {"properties": {
                "extensions": {"properties": {
                    "dev.sailry.platform": {"$ref": EXTENSION_ID}
                }}
            }}
        ]
    })
}
