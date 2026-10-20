use super::*;
use adk_core::{Content, GenerateContentConfig};

#[test]
fn preserves_options_for_each_request() {
    let mut request = LlmRequest::new("fixture", vec![Content::new("user").with_text("Task")]);
    request.config = Some(GenerateContentConfig {
        temperature: Some(0.5),
        extensions: serde_json::Map::from_iter([
            (
                "openai".into(),
                json!({"prompt_cache_retention":"24h","reasoning":{"effort":"high"}}),
            ),
            ("other".into(), json!({"retained":true})),
        ]),
        ..Default::default()
    });
    let original = request.config.clone().unwrap();
    let mut ordinary = request.clone();
    configure(
        &mut request,
        &BTreeMap::from([("speed".into(), "fast".into())]),
    )
    .unwrap();
    assert_eq!(
        request.config.as_ref().unwrap().extensions["openai"]["service_tier"],
        "priority"
    );
    assert_eq!(request.config.as_ref().unwrap().temperature, Some(0.5));
    configure(&mut ordinary, &BTreeMap::new()).unwrap();
    assert_eq!(
        serde_json::to_value(ordinary.config.unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}

#[test]
fn leaves_unselected_requests_unchanged() {
    let mut request = LlmRequest::new("fixture", Vec::new());
    configure(&mut request, &BTreeMap::new()).unwrap();
    assert!(request.config.is_none());
    request.config = Some(GenerateContentConfig {
        extensions: serde_json::Map::from_iter([("openai".into(), json!({"service_tier":"auto"}))]),
        ..Default::default()
    });
    let original = request.config.clone();
    configure(&mut request, &BTreeMap::new()).unwrap();
    assert_eq!(
        serde_json::to_value(request.config).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}

#[test]
fn rejects_incompatible_options() {
    let mut request = LlmRequest::new("fixture", Vec::new());
    request.config = Some(GenerateContentConfig {
        extensions: serde_json::Map::from_iter([("openai".into(), json!(false))]),
        ..Default::default()
    });
    assert!(
        configure(
            &mut request,
            &BTreeMap::from([("speed".into(), "fast".into())])
        )
        .is_err()
    );
}
