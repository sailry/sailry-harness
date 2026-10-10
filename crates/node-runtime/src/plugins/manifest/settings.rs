use super::*;
use serde_json::json;

#[test]
fn requires_declared_settings_resources() {
    for name in ["code", "web-search", "custom"] {
        let mut value = json!({"$schema":SCHEMA,"name":name,
        "extensions":{"dev.sailry.platform":{"api_version":"v1",
            "settings_page":{"navigation":{"label":"Settings"},"entry":"dev.sailry.platform/settings.js"},
            "desktop":{"entry":null,"resources":["dev.sailry.platform/settings.js"]}
        }}});
        assert!(
            parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .issues
                .is_empty()
        );
        value["extensions"]["dev.sailry.platform"]["settings_page"]["entry"] =
            json!("dev.sailry.platform/missing.js");
        assert!(
            parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .extension
                .is_none()
        );
    }
}
