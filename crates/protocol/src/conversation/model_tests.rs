use super::*;
use serde_json::json;

#[test]
fn missing_capabilities_are_disabled() {
    let value = json!({"id":"fixture","context":4096,"output":1024,"future_metadata":true});
    let model: Model = serde_json::from_value(value).unwrap();
    assert!(!model.vision);
    assert!(!model.tools);
    assert!(!model.reasoning);
    assert!(!model.web_search);
    assert!(model.generates.is_empty());
    assert!(model.efforts.is_empty());
    assert!(!model.custom_efforts);
    assert_eq!(model.default_effort, crate::Effort::Default);
}

#[test]
fn identity_and_capacity_remain_required() {
    let value = json!({"id":"fixture","context":4096,"output":1024});
    for field in ["id", "context", "output"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Model>(missing).is_err());
    }
    for field in ["vision", "tools", "reasoning", "web_search"] {
        let mut malformed = value.clone();
        malformed[field] = json!("true");
        assert!(serde_json::from_value::<Model>(malformed).is_err());
    }
}
