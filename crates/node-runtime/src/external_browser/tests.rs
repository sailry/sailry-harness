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
    assert_eq!(extension.tools.len(), 49);
    let mut actions = std::collections::BTreeSet::new();
    let mut reads = 0;
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
        reads += usize::from(action.read_only());
    }
    assert_eq!(reads, 19);
    let native =
        adk_browser::BrowserToolset::new(Arc::new(BrowserSession::new(BrowserConfig::default())))
            .all_tools();
    assert_eq!(native.len(), 46);
    for tool in native {
        let declaration = extension
            .tools
            .iter()
            .find(|declaration| declaration.name == tool.name())
            .unwrap();
        assert_eq!(
            Some(declaration.handler.as_ref().unwrap().parameters.clone()),
            tool.parameters_schema(),
            "{}",
            tool.name()
        );
    }
    for name in [
        "browser_close_session",
        "browser_downloads",
        "browser_save_download",
    ] {
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
