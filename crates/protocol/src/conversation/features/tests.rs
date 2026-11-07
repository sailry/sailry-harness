use super::*;
use serde_json::json;

fn model() -> Model {
    serde_json::from_value(json!({"id":"fixture","context":4096,"output":512,
        "capabilities":{"vision":null,"tools":null,"reasoning":null,"efforts":null,"default_effort":null,
            "features":[{"id":"speed","name":"Speed","icon":"reicon:newicons/flash","options":[{"id":"fast","name":"Fast"},{"id":"ultrafast","name":"Ultra fast"}]}]}})).unwrap()
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "separates_choices_from_transport_support",
            (|scenario: &'static str| {
                let model = model();
                let fast = BTreeMap::from([("speed".into(), "fast".into())]);
                assert!(
                    model.validate_features(ModelApi::Responses, &fast).is_ok(),
                    "{scenario}"
                );
                assert!(
                    model.validate_features(ModelApi::Plugin, &fast).is_ok(),
                    "{scenario}"
                );
                assert!(
                    model.validate_features(ModelApi::Anthropic, &fast).is_err(),
                    "{scenario}"
                );
                let ultrafast = BTreeMap::from([("speed".into(), "ultrafast".into())]);
                for api in [ModelApi::Plugin, ModelApi::Responses] {
                    assert!(
                        model.validate_features(api, &ultrafast).is_ok(),
                        "{scenario}"
                    );
                }
                assert!(
                    model
                        .validate_features(ModelApi::Anthropic, &ultrafast)
                        .is_err(),
                    "{scenario}"
                );
                let other = BTreeMap::from([("speed".into(), "turbo".into())]);
                let mut declared = model.clone();
                declared.capabilities.as_mut().unwrap().features[0]
                    .options
                    .push(FeatureOption {
                        id: "turbo".into(),
                        name: None,
                        icon: None,
                    });
                assert!(
                    declared
                        .validate_features(ModelApi::Responses, &other)
                        .is_err(),
                    "{scenario}"
                );
                let mut selected = BTreeMap::from([
                    ("speed".into(), "fast".into()),
                    ("unknown".into(), "value".into()),
                ]);
                model.retain_features(ModelApi::Responses, &mut selected);
                assert_eq!(selected, fast, "{scenario}");
                model.retain_features(ModelApi::Gemini, &mut selected);
                assert!(selected.is_empty(), "{scenario}");
                for selected in [
                    BTreeMap::from([("unknown".into(), "fast".into())]),
                    BTreeMap::from([("speed".into(), "unknown".into())]),
                ] {
                    assert!(
                        model
                            .validate_features(ModelApi::Plugin, &selected)
                            .is_err(),
                        "{scenario}"
                    );
                }
            }) as fn(&'static str),
        ),
        (
            "rejects_conflicting_declarations",
            (|scenario: &'static str| {
                let mut model = model();
                let features = &mut model.capabilities.as_mut().unwrap().features;
                assert!(validate(features).is_ok(), "{scenario}");
                features.push(features[0].clone());
                assert!(validate(features).is_err(), "{scenario}");
                features.pop();
                let duplicate = features[0].options[0].clone();
                features[0].options.push(duplicate);
                assert!(validate(features).is_err(), "{scenario}");
                features[0].options.pop();
                features[0].options[1].icon =
                    Some(Icon::Name("reicon:newicons/flash-circle".into()));
                assert!(validate(features).is_ok(), "{scenario}");
                features[0].options[1].icon = Some(Icon::Name("not-an-embedded-icon".into()));
                assert!(validate(features).is_err(), "{scenario}");
                features[0].options[1].icon = None;
                features[0].icon = Some(Icon::Name("not-an-embedded-icon".into()));
                assert!(validate(features).is_err(), "{scenario}");
            }),
        ),
        (
            "preserves_absent_configuration_fields",
            (|scenario: &'static str| {
                let value = json!({"provider":crate::ProviderId::new(),"model":"fixture","effort":"high","mode":"code","options":{},"permission":"ask","credential":null});
                let mut config: crate::SessionConfig =
                    serde_json::from_value(value.clone()).unwrap();
                assert!(config.features.is_empty(), "{scenario}");
                assert_eq!(serde_json::to_value(&config).unwrap(), value, "{scenario}");
                config.features.insert("speed".into(), "fast".into());
                assert_eq!(
                    serde_json::to_value(&config).unwrap()["features"],
                    json!({"speed":"fast"}),
                    "{scenario}"
                );
                assert_eq!(config.effort, crate::Effort::High, "{scenario}");
                config.features.clear();
                assert_eq!(serde_json::to_value(&config).unwrap(), value, "{scenario}");
                let before = json!({"vision":null,"tools":null,"reasoning":null,"efforts":null,"default_effort":null});
                let capabilities: super::super::discovery::Capabilities =
                    serde_json::from_value(before.clone()).unwrap();
                assert!(capabilities.features.is_empty(), "{scenario}");
                assert_eq!(
                    serde_json::to_value(capabilities).unwrap(),
                    before,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
