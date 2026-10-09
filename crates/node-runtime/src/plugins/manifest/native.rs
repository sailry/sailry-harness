use super::*;
use sailry_protocol::plugin::desktop::NativeSettings;
use serde_json::json;

fn manifest(name: &str, native: &str) -> Value {
    json!({
        "$schema": SCHEMA, "name":name,
        "extensions":{"dev.sailry.platform":{
            "api_version":"v1", "settings_page":{
                "navigation":{"label":"Settings"}, "native":native, "entry":null,
                "enabled_control":true, "placement":{"group":"tools","order":300}
            }
        }}
    })
}

#[test]
fn accepts_corresponding_bundled_renderers() {
    for (name, id, native) in [
        ("code", "code", NativeSettings::Code),
        ("web-search", "search", NativeSettings::Search),
    ] {
        let parsed = parse(&serde_json::to_vec(&manifest(name, id)).unwrap()).unwrap();
        assert!(parsed.issues.is_empty());
        let extension = parsed.extension.unwrap();
        assert!(extension.desktop.is_none());
        let page = extension.settings_page.unwrap();
        assert_eq!(page.native, Some(native));
        assert!(page.entry.is_none());
        assert!(page.enabled_control);
    }
}

#[test]
fn rejects_mismatched_packages_and_unknown_renderers() {
    for (name, native) in [
        ("code", "search"),
        ("web-search", "code"),
        ("other", "code"),
        ("other", "search"),
        ("code", "unknown"),
    ] {
        let parsed = parse(&serde_json::to_vec(&manifest(name, native)).unwrap()).unwrap();
        assert!(parsed.extension.is_none(), "accepted {name}/{native}");
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].kind, IssueKind::InvalidExtensions);
    }
}

#[test]
fn rejects_simultaneous_script_entries() {
    for (name, native) in [("code", "code"), ("web-search", "search")] {
        let mut value = manifest(name, native);
        let extension = &mut value["extensions"]["dev.sailry.platform"];
        extension["settings_page"]["entry"] = json!("dev.sailry.platform/settings.js");
        extension["desktop"] = json!({
            "entry":null, "resources":["dev.sailry.platform/settings.js"]
        });
        let parsed = parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(parsed.extension.is_none());
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].kind, IssueKind::InvalidExtensions);
        assert_eq!(
            parsed.issues[0].path,
            "plugin.json.extensions.dev.sailry.platform.settings_page"
        );
    }
}
