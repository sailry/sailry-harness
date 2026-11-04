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
fn bounded_variants() {
    for (scenario, check) in [
        (
            "accepts_optional_motion",
            (|_scenario: &str| {
                assert!(
                    validate(&BTreeMap::new(), &BTreeMap::new()).is_ok(),
                    "accepts optional motion"
                );
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
                    assert!(
                        validate(&layers, &BTreeMap::new()).is_ok(),
                        "accepts optional motion"
                    );
                }
            }) as fn(&str),
        ),
        (
            "rejects_invalid_dimensions",
            (|_scenario: &str| {
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
                        "rejects invalid dimensions: accepted invalid {key}"
                    );
                }
                let mut layer = effect();
                layer.opacity = f32::NAN;
                assert!(
                    validate(
                        &BTreeMap::from([("conversation".into(), vec![layer])]),
                        &BTreeMap::new()
                    )
                    .is_err(),
                    "rejects invalid dimensions"
                );
            }),
        ),
        (
            "rejects_unknown_fields",
            (|_scenario: &str| {
                let original = serde_json::to_value(effect()).unwrap();
                for (key, replacement) in [
                    ("kind", json!("script")),
                    ("script", json!("run()")),
                    ("count", json!(256)),
                ] {
                    let mut value = original.clone();
                    value[key] = replacement;
                    assert!(
                        serde_json::from_value::<Effect>(value).is_err(),
                        "rejects unknown fields"
                    );
                }
            }),
        ),
        (
            "bounds_layers_and_particles",
            (|_scenario: &str| {
                let mut layer = effect();
                layer.count = 64;
                for (count, accepted) in [(3, true), (4, false), (5, false), (0, false)] {
                    let layers =
                        BTreeMap::from([("background.window".into(), vec![layer.clone(); count])]);
                    assert_eq!(
                        validate(&layers, &BTreeMap::new()).is_ok(),
                        accepted,
                        "bounds layers and particles"
                    );
                }
                for slot in [
                    "button.send",
                    "background.card.plan",
                    "background.unknown",
                    "script",
                ] {
                    let layers = BTreeMap::from([(slot.into(), vec![effect()])]);
                    assert!(
                        validate(&layers, &BTreeMap::new()).is_err(),
                        "bounds layers and particles"
                    );
                }
            }),
        ),
        (
            "requires_banner_artwork",
            (|_scenario: &str| {
                let value = crate::theme::fixture::manifest("tide");
                let manifest: super::super::Manifest = serde_json::from_value(value).unwrap();
                for slot in ["hero.new_session", "banner.conversation.top"] {
                    let layers = BTreeMap::from([(slot.into(), vec![effect()])]);
                    assert!(
                        validate(&layers, &BTreeMap::new()).is_err(),
                        "requires banner artwork"
                    );
                    assert!(
                        validate(&layers, &manifest.light.assets).is_ok(),
                        "requires banner artwork"
                    );
                }
            }),
        ),
        (
            "checks_each_variant",
            (|_scenario: &str| {
                for mode in ["light", "dark"] {
                    let mut value = crate::theme::fixture::manifest("tide");
                    value[mode]["effects"] = json!({"conversation": [effect()]});
                    let manifest: super::super::Manifest =
                        serde_json::from_value(value.clone()).unwrap();
                    manifest.validate().unwrap();
                    value[mode]["effects"]["conversation"][0]["duration"] = json!(0);
                    let manifest: super::super::Manifest = serde_json::from_value(value).unwrap();
                    assert!(manifest.validate().is_err(), "checks each variant");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
