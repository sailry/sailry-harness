use super::*;
use sailry_protocol::{external_browser::Action, plugin::Extension, tool::Operation};
use serde_json::{Value, json};

#[test]
fn package_preserves_actions_and_schemas() {
    let manifest: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../plugins/external-browser/plugin.json"
    )))
    .unwrap();
    let extension: Extension =
        serde_json::from_value(manifest["extensions"]["dev.sailry.platform"].clone()).unwrap();
    let mut actions = std::collections::BTreeSet::new();
    for declaration in &extension.tools {
        let handler = declaration.handler.as_ref().unwrap();
        let action: Action = serde_json::from_value(json!(handler.name)).unwrap();
        assert!(actions.insert(handler.name.clone()));
        assert_eq!(declaration.name, format!("browser_{}", handler.name));
        assert_eq!(
            handler.operation,
            Some(if action.read_only() {
                Operation::ReadExternalBrowser
            } else {
                Operation::ControlExternalBrowser
            })
        );
    }
    // Frozen from the reviewed upstream parameter declarations; no Agent toolset is needed.
    let native: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(include_str!("tests/schemas.json")).unwrap();
    let local = [
        "browser_close_session",
        "browser_downloads",
        "browser_save_download",
    ];
    let expected = native
        .keys()
        .map(String::as_str)
        .chain(local)
        .collect::<std::collections::BTreeSet<_>>();
    let declared = extension
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(declared, expected);
    for (name, schema) in native {
        let declaration = extension
            .tools
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap();
        let mut declared = declaration.handler.as_ref().unwrap().parameters.clone();
        let mut native = schema;
        // Descriptions can document the local adapter; validation constraints stay identical.
        strip_descriptions(&mut declared);
        strip_descriptions(&mut native);
        assert_eq!(declared, native, "{name}");
    }
    for name in local {
        let declaration = extension
            .tools
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap();
        let expected = if name == "browser_save_download" {
            json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"],"additionalProperties":false})
        } else {
            json!({"type":"object","properties":{},"required":[],"additionalProperties":false})
        };
        assert_eq!(declaration.handler.as_ref().unwrap().parameters, expected);
    }
}

fn strip_descriptions(value: &mut Value) {
    match value {
        Value::Object(values) => {
            values.remove("description");
            for value in values.values_mut() {
                strip_descriptions(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_descriptions(value);
            }
        }
        _ => {}
    }
}

#[test]
fn validates_unclamped_wait_durations() {
    for seconds in [0.0, 0.25, 31.0, 60.0] {
        validate(Action::Wait, &json!({"seconds":seconds})).unwrap();
        assert_eq!(
            execute::wait_duration(seconds).unwrap().as_secs_f64(),
            seconds
        );
    }
    for seconds in [-1.0, 1e300, f64::INFINITY, f64::NAN] {
        assert!(execute::wait_duration(seconds).is_err());
    }
}
