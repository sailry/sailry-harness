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

#[test]
fn groups_builtin_methods_by_type() {
    let methods = [
        ("api_key", Some("openai")),
        ("chat_gpt_oauth", None),
        ("chat_gpt", Some("chat-gpt")),
        ("api_key", None),
    ];
    let groups = methods
        .into_iter()
        .enumerate()
        .map(|(id, (authentication, preset))| {
            let provider: Provider = serde_json::from_value(serde_json::json!({
                "id":sailry_protocol::ProviderId::new(), "revision":1,
                "name":format!("Channel {id}"), "api":"responses",
                "authentication":authentication, "endpoint":"https://example.test/v1",
                "enabled":true, "default_model":"shared",
                "preset":preset.map(|id| PresetRef {
                    source:Source::Builtin, id:id.into(), route:None
                })
            }))
            .unwrap();
            let mut channel = crate::settings::channel(id, &provider);
            channel.models = vec![crate::settings::Model::example("shared")];
            Group::from(channel)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        groups
            .iter()
            .map(|group| &group.family)
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
    for (id, group) in groups.iter().enumerate() {
        assert_eq!(group.family, "openai");
        assert_eq!(group.id, id);
        assert_eq!(group.name, format!("Channel {id}"));
        assert_eq!(group.models[0].id, "shared");
        assert_eq!(
            crate::settings::type_label(group.directory.as_ref().unwrap()),
            "OpenAI"
        );
    }
    let provider: Provider = serde_json::from_value(serde_json::json!({
        "id":sailry_protocol::ProviderId::new(), "revision":1,"name":"Other channel",
        "api":"anthropic","authentication":"claude_oauth",
        "endpoint":"https://example.test", "enabled":true,"default_model":"shared"
    }))
    .unwrap();
    let other = Group::from(crate::settings::channel(4, &provider));
    assert_eq!(other.family, "anthropic");
    assert_ne!(other.family, groups[0].family);
}
