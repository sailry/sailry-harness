use super::*;
use serde_json::json;

fn extension() -> plugin::Extension {
    serde_json::from_value(json!({"api_version":"v1","conversation_options":{
        "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{"enabled":{"type":"boolean","default":false}},"required":["enabled"]
    }}))
    .unwrap()
}

#[test]
fn validates_values_without_exposing_them() {
    let extension = extension();
    assert_eq!(project(&extension, None).unwrap()["enabled"], json!(false));
    assert_eq!(
        project(
            &extension,
            Some(&BTreeMap::from([("enabled".into(), json!(true))]))
        )
        .unwrap()["enabled"],
        json!(true)
    );
    for values in [
        json!({"enabled":"private-fixture-input"}),
        json!({"unknown":true}),
    ] {
        let values = serde_json::from_value(values).unwrap();
        let error = project(&extension, Some(&values)).unwrap_err();
        assert_eq!(error.code, sailry_protocol::ErrorCode::InvalidRequest);
        assert!(!error.message.contains("private-fixture-input"));
    }
}

#[test]
fn rejects_privileged_option_declarations() {
    for field in [
        json!({"type":"string","x-sailry-secret":{"origin":"https://example.test","header":"authorization"}}),
        json!({"type":"string","x-sailry-model":true}),
        json!({"type":"string","default":"8","x-sailry-child-concurrency":true}),
    ] {
        let mut extension = extension();
        extension.conversation_options.as_mut().unwrap()["properties"]["enabled"] = field;
        assert!(schema(&extension).is_err());
    }
}

#[test]
fn ignores_unowned_and_retired_values() {
    let mut info: plugin::Info = serde_json::from_value(json!({"summary":{"name":"example","revision":1,"settings_revision":0,"enabled":true,"digest":"a".repeat(64)}})).unwrap();
    info.extension = Some(extension());
    let config: SessionConfig = serde_json::from_value(json!({"provider":sailry_protocol::ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null,
        "options":{"example":{"enabled":true,"retired":"preserved"},"unrelated":{"enabled":false}}})).unwrap();
    assert_eq!(
        captured(&info, &config).unwrap(),
        BTreeMap::from([("enabled".into(), json!(true))])
    );
    info.extension.as_mut().unwrap().conversation_options = None;
    assert!(captured(&info, &config).unwrap().is_empty());
    assert_eq!(config.options["example"]["retired"], json!("preserved"));
}
