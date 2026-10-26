use super::*;
use sailry_protocol::{Permission, ProviderId};
use serde_json::json;

fn modes() -> [Mode; 2] {
    [("first", true), ("second", false)].map(|(name, write)| Mode {
        package: serde_json::from_value(json!({
            "name":name,"revision":1,"settings_revision":0,"digest":"a".repeat(64),
        }))
        .unwrap(),
        declaration: serde_json::from_value(json!({
            "id":"review","option":"mode","display":{"label":"Review"},
            "permissions":{"write":write},"instructions":"Inspect changes",
        }))
        .unwrap(),
        enabled: true,
    })
}

fn config() -> SessionConfig {
    serde_json::from_value(json!({
        "provider":ProviderId::new(),"model":"fixture","mode":"code","permission":"ask","credential":null,
        "options":{"first":{"proactive":true,"retired":42},"second":{"unrelated":"keep"},"other":{"value":true}},
    }))
    .unwrap()
}

#[test]
fn switches_owners_and_returns_to_base() {
    let modes = modes();
    let original = config();
    let mut config = original.clone();
    for mode in &modes {
        config = select(&config, &modes, &mode.key()).unwrap();
        assert_eq!(project(&config, &modes).selected, Some(mode.key()));
        assert_eq!(config.mode, mode.declaration.base());
        assert_eq!(config.permission, Permission::Ask);
    }
    assert_eq!(config.options["first"]["mode"], json!(""));
    for base in [WorkMode::Code, WorkMode::Plan] {
        config = select(&config, &modes, key(base)).unwrap();
        assert_eq!(
            project(&config, &modes).selected.as_deref(),
            Some(key(base))
        );
        assert_eq!(config.mode, base);
        assert_eq!(config.options["second"]["mode"], json!(""));
    }
    for (owner, values) in &original.options {
        for (key, value) in values {
            assert_eq!(&config.options[owner][key], value);
        }
    }
    assert!(!original.options["first"].contains_key("mode"));
}

#[test]
fn rejects_ambiguous_and_incompatible_markers() {
    let modes = modes();
    let mut config = select(&config(), &modes, &modes[1].key()).unwrap();
    config
        .options
        .get_mut("first")
        .unwrap()
        .insert("mode".into(), json!("review"));
    assert!(project(&config, &modes).selected.is_none());
    let cleared = select(&config, &modes, "plan").unwrap();
    assert_eq!(project(&cleared, &modes).selected.as_deref(), Some("plan"));
    config
        .options
        .get_mut("first")
        .unwrap()
        .insert("mode".into(), json!(""));
    config.mode = WorkMode::Code;
    assert!(project(&config, &modes).selected.is_none());
    assert!(select(&config, &modes, "missing/review").is_none());
}

#[test]
fn recognizes_and_clears_disabled_selections() {
    let mut modes = modes();
    let selected = select(&config(), &modes, &modes[0].key()).unwrap();
    modes[0].enabled = false;
    let view = project(&selected, &modes);
    assert_eq!(view.selected, Some(modes[0].key()));
    assert!(view.choices.contains(&modes[0].key()));
    assert!(select(&selected, &modes, &modes[0].key()).is_none());
    let cleared = select(&selected, &modes, "code").unwrap();
    assert!(!project(&cleared, &modes).choices.contains(&modes[0].key()));
    assert_eq!(cleared.options["first"]["proactive"], json!(true));
}

#[test]
fn preserves_options_without_installed_declarations() {
    let modes = modes();
    let selected = select(&config(), &modes, &modes[0].key()).unwrap();
    let cleared = select(&selected, &[], "code").unwrap();
    assert_eq!(cleared.options, selected.options);
    assert_eq!(project(&cleared, &[]).selected.as_deref(), Some("code"));
}
