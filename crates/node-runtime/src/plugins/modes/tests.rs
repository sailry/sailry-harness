use super::*;
use sailry_protocol::{Permission, ProviderId, WorkMode};
use serde_json::json;

pub(crate) fn package(name: &str, id: &str, write: bool) -> plugin::Info {
    serde_json::from_value(json!({
        "summary":{"name":name,"revision":1,"settings_revision":0,"enabled":true,"digest":blake3::hash(name.as_bytes()).to_hex().to_string()},
        "extension":{"api_version":"v1","conversation_options":{
            "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
            "properties":{
                "mode":{"type":"string","default":"","enum":["",id]},
                "proactive":{"type":"boolean","default":false}
            }
        },"conversation_modes":[{
            "id":id,"option":"mode","display":{"label":id},
            "permissions":{"write":write},"instructions":"Use the selected work mode"
        }]}
    })).unwrap()
}

pub(crate) fn config() -> SessionConfig {
    serde_json::from_value(json!({
        "provider":ProviderId::new(),"model":"fixture","mode":"code",
        "permission":"ask","credential":null
    }))
    .unwrap()
}

#[test]
fn requires_declared_empty_default() {
    let info = package("example", "review", false);
    let original = info.extension.unwrap();
    assert!(valid(&original));
    for value in [
        json!({"type":"string","default":"review","enum":["","review"]}),
        json!({"type":"string","default":"","enum":["review"]}),
        json!({"type":"string","default":"","enum":["","review","unknown"]}),
        json!({"type":"boolean","default":false}),
    ] {
        let mut extension = original.clone();
        extension.conversation_options.as_mut().unwrap()["properties"]["mode"] = value;
        assert!(!valid(&extension));
    }
    let mut extension = original;
    extension.scope = plugin::Scope::Desktop;
    assert!(!valid(&extension));
}

#[test]
fn rejects_ambiguous_package_choices() {
    let packages = [
        package("first", "review", false),
        package("second", "review", false),
    ];
    let mut config = config();
    config.mode = WorkMode::Plan;
    config.options = serde_json::from_value(json!({
        "first":{"mode":"review"},"second":{"mode":""}
    }))
    .unwrap();
    let (owner, mode) = selected(&packages, &config).unwrap().unwrap();
    assert_eq!(owner.summary.name, "first");
    assert_eq!(mode.id, "review");
    config
        .options
        .get_mut("second")
        .unwrap()
        .insert("mode".into(), json!("review"));
    assert_eq!(
        selected(&packages, &config).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    config
        .options
        .get_mut("first")
        .unwrap()
        .insert("mode".into(), json!(""));
    assert_eq!(
        selected(&packages, &config)
            .unwrap()
            .unwrap()
            .0
            .summary
            .name,
        "second"
    );
}

#[test]
fn bounds_authority_with_effective_mode() {
    let packages = [package("example", "review", false)];
    for permission in [Permission::Ask, Permission::Project, Permission::Full] {
        let mut config = config();
        config.permission = permission;
        config.options = serde_json::from_value(json!({"example":{"mode":"review"}})).unwrap();
        assert_eq!(
            selected(&packages, &config).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        config.mode = WorkMode::Plan;
        assert!(selected(&packages, &config).unwrap().is_some());
        assert_eq!(config.permission, permission);
    }
    let packages = [package("example", "multitask", true)];
    let mut config = config();
    config.options = serde_json::from_value(json!({"example":{"mode":"multitask"}})).unwrap();
    for mode in [WorkMode::Code, WorkMode::Plan] {
        config.mode = mode;
        assert!(selected(&packages, &config).unwrap().is_some());
        assert_eq!(config.permission, Permission::Ask);
    }
}

#[test]
fn retained_options_do_not_select_modes() {
    let packages = [package("example", "multitask", true)];
    let mut config = config();
    config.options =
        serde_json::from_value(json!({"example":{"proactive":true,"retired":true}})).unwrap();
    assert!(selected(&packages, &config).unwrap().is_none());
    config
        .options
        .get_mut("example")
        .unwrap()
        .insert("mode".into(), json!("multitask"));
    assert!(selected(&packages, &config).unwrap().is_some());
    assert!(selected(&[], &config).unwrap().is_none());
    assert_eq!(config.options["example"]["retired"], json!(true));
}
