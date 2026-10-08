use super::*;
use serde_json::json;

fn effect() -> Effect {
    serde_json::from_value(json!({
        "kind": "twinkle", "color": "#80DFFF", "count": 12,
        "opacity": 0.2, "size": 2.0, "duration": 12.0
    }))
    .unwrap()
}

#[test]
fn accepts_optional_motion() {
    assert!(validate(&BTreeMap::new(), &BTreeMap::new()).is_ok());
    for kind in [
        Kind::Twinkle,
        Kind::Embers,
        Kind::Dust,
        Kind::Petals,
        Kind::Pulse,
        Kind::Scan,
    ] {
        let mut layer = effect();
        layer.kind = kind;
        layer.count = 1;
        let layers = BTreeMap::from([("background.window".into(), vec![layer])]);
        assert!(validate(&layers, &BTreeMap::new()).is_ok());
    }
}

#[test]
fn rejects_invalid_dimensions() {
    let original = serde_json::to_value(effect()).unwrap();
    for (key, replacement) in [
        ("count", json!(0)),
        ("count", json!(65)),
        ("opacity", json!(-0.1)),
        ("opacity", json!(1.1)),
        ("size", json!(0.1)),
        ("size", json!(13)),
        ("duration", json!(0)),
        ("duration", json!(121)),
        ("color", json!("not-a-color")),
        ("kind", json!("pulse")),
        ("kind", json!("scan")),
    ] {
        let mut value = original.clone();
        value[key] = replacement;
        let layer: Effect = serde_json::from_value(value).unwrap();
        let layers = BTreeMap::from([("background.window".into(), vec![layer])]);
        assert!(
            validate(&layers, &BTreeMap::new()).is_err(),
            "accepted invalid {key}"
        );
    }
    let mut layer = effect();
    layer.opacity = f32::NAN;
    assert!(
        validate(
            &BTreeMap::from([("conversation".into(), vec![layer])]),
            &BTreeMap::new()
        )
        .is_err()
    );
}

#[test]
fn rejects_unknown_fields() {
    let original = serde_json::to_value(effect()).unwrap();
    for (key, replacement) in [
        ("kind", json!("script")),
        ("script", json!("run()")),
        ("count", json!(256)),
    ] {
        let mut value = original.clone();
        value[key] = replacement;
        assert!(serde_json::from_value::<Effect>(value).is_err());
    }
}

#[test]
fn bounds_layers_and_particles() {
    let mut layer = effect();
    layer.count = 64;
    for (count, accepted) in [(3, true), (4, false), (5, false), (0, false)] {
        let layers = BTreeMap::from([("background.window".into(), vec![layer.clone(); count])]);
        assert_eq!(validate(&layers, &BTreeMap::new()).is_ok(), accepted);
    }
    for slot in [
        "button.send",
        "background.card.plan",
        "background.unknown",
        "script",
    ] {
        let layers = BTreeMap::from([(slot.into(), vec![effect()])]);
        assert!(validate(&layers, &BTreeMap::new()).is_err());
    }
}

#[test]
fn requires_banner_artwork() {
    let value = crate::theme::fixture::manifest("tide");
    let manifest: super::super::Manifest = serde_json::from_value(value).unwrap();
    for slot in ["hero.new_session", "banner.conversation.top"] {
        let layers = BTreeMap::from([(slot.into(), vec![effect()])]);
        assert!(validate(&layers, &BTreeMap::new()).is_err());
        assert!(validate(&layers, &manifest.light.assets).is_ok());
    }
}

#[test]
fn checks_each_variant() {
    for mode in ["light", "dark"] {
        let mut value = crate::theme::fixture::manifest("tide");
        value[mode]["effects"] = json!({"conversation": [effect()]});
        let manifest: super::super::Manifest = serde_json::from_value(value.clone()).unwrap();
        manifest.validate().unwrap();
        value[mode]["effects"]["conversation"][0]["duration"] = json!(0);
        let manifest: super::super::Manifest = serde_json::from_value(value).unwrap();
        assert!(manifest.validate().is_err());
    }
}
