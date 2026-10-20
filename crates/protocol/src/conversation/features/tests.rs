use super::*;
use serde_json::json;

fn model() -> Model {
    serde_json::from_value(json!({"id":"fixture","context":4096,"output":512,
        "capabilities":{"vision":null,"tools":null,"web_search":null,"reasoning":null,"efforts":null,"default_effort":null,
            "features":[{"id":"speed","name":"Speed","icon":"reicon:newicons/flash","options":[{"id":"fast","name":"Fast"},{"id":"ultrafast","name":"Ultra fast"}]}]}})).unwrap()
}

#[test]
fn separates_choices_from_transport_support() {
    let model = model();
    let fast = BTreeMap::from([("speed".into(), "fast".into())]);
    assert!(model.validate_features(ModelApi::Responses, &fast).is_ok());
    assert!(model.validate_features(ModelApi::Plugin, &fast).is_ok());
    assert!(model.validate_features(ModelApi::Anthropic, &fast).is_err());
    let future = BTreeMap::from([("speed".into(), "ultrafast".into())]);
    assert!(model.validate_features(ModelApi::Plugin, &future).is_ok());
    assert!(
        model
            .validate_features(ModelApi::Responses, &future)
            .is_err()
    );
    let mut selected = BTreeMap::from([
        ("speed".into(), "fast".into()),
        ("unknown".into(), "value".into()),
    ]);
    model.retain_features(ModelApi::Responses, &mut selected);
    assert_eq!(selected, fast);
    model.retain_features(ModelApi::Gemini, &mut selected);
    assert!(selected.is_empty());
    for selected in [
        BTreeMap::from([("unknown".into(), "fast".into())]),
        BTreeMap::from([("speed".into(), "unknown".into())]),
    ] {
        assert!(
            model
                .validate_features(ModelApi::Plugin, &selected)
                .is_err()
        );
    }
}

#[test]
fn rejects_conflicting_declarations() {
    let mut model = model();
    let features = &mut model.capabilities.as_mut().unwrap().features;
    assert!(validate(features).is_ok());
    features.push(features[0].clone());
    assert!(validate(features).is_err());
    features.pop();
    let duplicate = features[0].options[0].clone();
    features[0].options.push(duplicate);
    assert!(validate(features).is_err());
    features[0].options.pop();
    features[0].icon = Some(Icon::Name("not-an-embedded-icon".into()));
    assert!(validate(features).is_err());
}

#[test]
fn preserves_absent_configuration_fields() {
    let value = json!({"provider":crate::ProviderId::new(),"model":"fixture","effort":"high","mode":"code","options":{},"permission":"ask","credential":null});
    let mut config: crate::SessionConfig = serde_json::from_value(value.clone()).unwrap();
    assert!(config.features.is_empty());
    assert_eq!(serde_json::to_value(&config).unwrap(), value);
    config.features.insert("speed".into(), "fast".into());
    assert_eq!(
        serde_json::to_value(&config).unwrap()["features"],
        json!({"speed":"fast"})
    );
    assert_eq!(config.effort, crate::Effort::High);
    config.features.clear();
    assert_eq!(serde_json::to_value(&config).unwrap(), value);
    let before = json!({"vision":null,"tools":null,"web_search":null,"reasoning":null,"efforts":null,"default_effort":null});
    let capabilities: super::super::discovery::Capabilities =
        serde_json::from_value(before.clone()).unwrap();
    assert!(capabilities.features.is_empty());
    assert_eq!(serde_json::to_value(capabilities).unwrap(), before);
}
