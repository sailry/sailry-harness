//! Authoring is strict; loading still follows Agent Plugins' isolated failure rules.
use super::*;
use sailry_protocol::plugin::schema;
use serde_json::json;

const BASE: &str = include_str!("../../../../../schemas/agent-plugins/1.0.0/plugin.schema.json");
const EXTENSION: &str = include_str!("../../../../../schemas/plugins/v1/extension.schema.json");
const MANIFEST: &str = include_str!("../../../../../schemas/plugins/v1/plugin.schema.json");

struct NoRetrieval;
impl jsonschema::Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err("schema retrieval is disabled".into())
    }
}

fn validator() -> jsonschema::Validator {
    let registry = jsonschema::Registry::new()
        .add(
            schema::AGENT_PLUGINS_ID,
            serde_json::from_str::<Value>(BASE).unwrap(),
        )
        .unwrap()
        .add(
            schema::EXTENSION_ID,
            serde_json::from_str::<Value>(EXTENSION).unwrap(),
        )
        .unwrap()
        .prepare()
        .unwrap();
    jsonschema::options()
        .with_registry(&registry)
        .with_retriever(NoRetrieval)
        .build(&serde_json::from_str::<Value>(MANIFEST).unwrap())
        .unwrap()
}

#[test]
fn exports_match_the_deserialization_contract() {
    assert_eq!(
        schema::extension(),
        serde_json::from_str::<Value>(EXTENSION).unwrap()
    );
    assert_eq!(
        schema::manifest(),
        serde_json::from_str::<Value>(MANIFEST).unwrap()
    );
}

#[test]
fn renderer_shortcuts_preserve_strict_authoring() {
    let validator = validator();
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("../../../../../plugins/browser/plugin.json"))
            .unwrap();
    assert!(validator.is_valid(&manifest));
    let loaded = parse(&serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(loaded.issues.is_empty());
    let desktop = loaded.extension.unwrap().desktop.unwrap();
    let renderer = &desktop.renderers[0];
    assert_eq!(renderer.shortcut.as_deref(), Some("secondary-t"));

    let renderer = &mut manifest["extensions"]["dev.sailry.platform"]["desktop"]["renderers"][0];
    renderer.as_object_mut().unwrap().remove("shortcut");
    assert!(validator.is_valid(&manifest));
    for shortcut in [json!(null), json!("secondary-t")] {
        manifest["extensions"]["dev.sailry.platform"]["desktop"]["renderers"][0]["shortcut"] =
            shortcut;
        assert!(validator.is_valid(&manifest));
    }
    for shortcut in [json!(true), json!(1), json!([]), json!({})] {
        manifest["extensions"]["dev.sailry.platform"]["desktop"]["renderers"][0]["shortcut"] =
            shortcut;
        assert!(!validator.is_valid(&manifest));
    }
    manifest["extensions"]["dev.sailry.platform"]["desktop"]["renderers"][0]["shortcut"] =
        json!("secondary-t");
    manifest["extensions"]["dev.sailry.platform"]["desktop"]["renderers"][0]["unknown"] =
        json!(true);
    assert!(!validator.is_valid(&manifest));
}

#[test]
fn message_commands_require_a_host_route() {
    let mut value: Value =
        serde_json::from_slice(include_bytes!("../../../../../plugins/goals/plugin.json")).unwrap();
    let check = |value: &Value| parse(&serde_json::to_vec(value).unwrap()).unwrap();
    assert!(check(&value).issues.is_empty());
    value["extensions"]["dev.sailry.platform"]["host"]["commands"] = json!([]);
    let missing = check(&value);
    assert!(missing.extension.is_none());
    assert_eq!(missing.issues[0].kind, IssueKind::InvalidExtensions);
}

#[test]
fn validates_all_shipped_manifests_offline() {
    let validator = validator();
    for (name, files) in crate::plugins::bundled::PACKAGES {
        let bytes = files
            .iter()
            .find(|(path, _)| *path == "plugin.json")
            .unwrap()
            .1;
        let manifest: Value = serde_json::from_slice(bytes).unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&manifest)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
    for bytes in [
        include_bytes!("../../../../../examples/plugins/project-summary/plugin.json").as_slice(),
        include_bytes!("../../../../../examples/plugins/task-notes/plugin.json").as_slice(),
        include_bytes!("../../../../../examples/plugins/tool-content/plugin.json").as_slice(),
    ] {
        let manifest: Value = serde_json::from_slice(bytes).unwrap();
        assert!(validator.is_valid(&manifest), "{}", manifest["name"]);
    }
}

#[test]
fn rejects_unknown_nested_metadata() {
    let validator = validator();
    let original: Value = serde_json::from_slice(include_bytes!(
        "../../../../../plugins/commands/plugin.json"
    ))
    .unwrap();
    assert!(validator.is_valid(&original));
    for mutation in 0..4 {
        let mut value = original.clone();
        let extension = &mut value["extensions"]["dev.sailry.platform"];
        match mutation {
            0 => extension["tools"][0]["handler"]["goal"] = json!(true),
            1 => extension["tools"][0]["presentation"] = json!("hidden"),
            2 => extension["desktop"]["network"] = json!(true),
            3 => extension["unknown"] = json!(true),
            _ => unreachable!(),
        }
        assert!(!validator.is_valid(&value), "mutation {mutation}");
    }
}

#[test]
fn validates_service_manifests_offline() {
    let validator = validator();
    let mut checked = 0;
    for (name, files) in crate::plugins::bundled::PACKAGES
        .iter()
        .filter(|(name, _)| matches!(*name, "context7" | "github" | "code-review"))
    {
        let bytes = files
            .iter()
            .find(|(path, _)| *path == "plugin.json")
            .unwrap()
            .1;
        let manifest: Value = serde_json::from_slice(bytes).unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&manifest)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        checked += 1;
    }
    assert_eq!(checked, 3);
}

#[test]
fn enforces_the_current_authoring_profile() {
    let validator = validator();
    let ordinary = json!({"$schema":SCHEMA, "name":"portable", "version":"0.1.0"});
    assert!(validator.is_valid(&ordinary));
    let mut manifest = ordinary.clone();
    manifest["extensions"] = json!({"dev.sailry.platform":{"api_version":"v1", "actions":[]}});
    assert!(validator.is_valid(&manifest));
    for extension in [
        json!({"api_version":"v2", "actions":[]}),
        json!({"api_version":"v1", "actions":["unknown.action"]}),
        json!({"api_version":"v1", "actions":[], "unknown":true}),
        json!({"api_version":"v1", "actions":[], "desktop":{"entry":"desktop/main.js", "resources":["desktop/main.js"]}}),
    ] {
        manifest["extensions"]["dev.sailry.platform"] = extension;
        assert!(!validator.is_valid(&manifest));
    }
    let mut ignored = ordinary;
    ignored["unknown"] = true.into();
    assert!(!validator.is_valid(&ignored));
    let loaded = parse(&serde_json::to_vec(&ignored).unwrap()).unwrap();
    assert_eq!(loaded.issues[0].kind, IssueKind::IgnoredManifestField);
    assert!(
        jsonschema::options()
            .with_retriever(NoRetrieval)
            .build(&json!({
                "$ref":"https://schemas.invalid/unregistered.json"
            }))
            .is_err()
    );
}
