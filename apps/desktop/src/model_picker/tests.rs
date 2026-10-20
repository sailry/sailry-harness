use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::{Provider, presets::*};

#[test]
fn retains_declared_type_identity() {
    let info: sailry_protocol::plugin::Info = serde_json::from_value(serde_json::json!({
        "summary": {"name":"sample.models", "revision":1, "digest":"sample", "settings_revision":0,
            "enabled":true, "version":"0.1.0"},
        "icon":"data:image/svg+xml;base64,PHN2Zy8+",
        "extension": {"api_version":"v1", "model_providers":[{
            "id":"regional", "name":"Regional", "brand":"sample",
            "brand_display":{"label":"Sample", "icon":"reicon:messages/chat"},
            "routes":[]
        }]}
    }))
    .unwrap();
    let mut packages = std::collections::BTreeMap::from([(info.summary.name.clone(), info)]);
    let mut reference = PresetRef {
        source: Source::Package {
            name: "sample.models".into(),
        },
        id: "regional".into(),
        route: None,
    };
    let entry = crate::settings::installed_preset(Some(&reference), &packages).unwrap();
    let provider: Provider = serde_json::from_value(serde_json::json!({
        "id":sailry_protocol::ProviderId::new(), "revision":1, "name":"Configured channel",
        "api":"plugin", "authentication":"api_key", "endpoint":"https://example.com",
        "enabled":true, "default_model":""
    }))
    .unwrap();
    let mut channel = crate::settings::channel(0, &provider);
    channel.directory = Some(entry.clone());
    let group = Group::from(channel.clone());
    assert_eq!(group.family, "package:sample.models:sample");
    assert_eq!(group.directory, Some(entry));
    assert_eq!(group.category, ModelCategory::Other);
    assert_eq!(
        crate::settings::type_label(group.directory.as_ref().unwrap()),
        "Sample"
    );

    channel.directory.as_mut().unwrap().preset.id = "another-region".into();
    assert_eq!(Group::from(channel.clone()).family, group.family);
    channel.directory.as_mut().unwrap().source = Source::Package {
        name: "different.models".into(),
    };
    assert_ne!(Group::from(channel).family, group.family);
    packages.get_mut("sample.models").unwrap().summary.enabled = false;
    assert!(crate::settings::installed_preset(Some(&reference), &packages).is_none());
    packages.get_mut("sample.models").unwrap().summary.enabled = true;
    reference.id = "missing".into();
    assert!(crate::settings::installed_preset(Some(&reference), &packages).is_none());
}
