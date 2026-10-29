use super::*;
use serde_json::json;

#[test]
fn missing_capabilities_are_disabled() {
    let value = json!({"id":"fixture","context":4096,"output":1024,"future_metadata":true});
    let model: Model = serde_json::from_value(value).unwrap();
    assert!(model.capabilities.is_none());
    assert!(!model.vision);
    assert!(!model.tools);
    assert!(!model.reasoning);
    assert!(
        serde_json::to_value(&model)
            .unwrap()
            .get("web_search")
            .is_none()
    );
    assert!(model.generates.is_empty());
    assert!(model.efforts.is_empty());
    assert!(!model.custom_efforts);
    assert_eq!(model.default_effort, crate::Effort::Default);
    assert_eq!(model.compaction_tokens, None);
}

#[test]
fn preserves_configured_compaction_threshold() {
    let value = json!({
        "id": "fixture",
        "context": 1_050_000,
        "output": 128_000,
        "compaction_tokens": 200_000,
    });
    let model: Model = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(model.compaction_tokens, Some(200_000));
    assert_eq!(
        serde_json::to_value(model).unwrap()["compaction_tokens"],
        value["compaction_tokens"]
    );
}

#[test]
fn preserves_native_capability_observations() {
    let model: Model = serde_json::from_value(json!({
        "id": "fixture",
        "context": 4096,
        "output": 1024,
        "capabilities": {"vision": false, "tools": true},
    }))
    .unwrap();
    let native = model.capabilities.as_ref().unwrap();
    assert_eq!(native.vision, Some(false));
    assert_eq!(native.tools, Some(true));
    assert_eq!(native.reasoning, None);
    assert_eq!(
        serde_json::to_value(model).unwrap()["capabilities"]["vision"],
        false
    );
}

#[test]
fn identity_and_capacity_remain_required() {
    let value = json!({"id":"fixture","context":4096,"output":1024});
    for field in ["id", "context", "output"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Model>(missing).is_err());
    }
    for field in ["vision", "tools", "reasoning"] {
        let mut malformed = value.clone();
        malformed[field] = json!("true");
        assert!(serde_json::from_value::<Model>(malformed).is_err());
    }
}

#[test]
fn display_name_does_not_change_execution_identity() {
    let model: Model = serde_json::from_value(json!({
        "id": "gpt-6.1-sol",
        "display_name": "GPT-6.1 Sol",
        "context": 4096,
        "output": 1024,
    }))
    .unwrap();
    assert_eq!(model.id, "gpt-6.1-sol");
    assert_eq!(model.display_name.as_deref(), Some("GPT-6.1 Sol"));
    assert_eq!(
        serde_json::to_value(model).unwrap()["display_name"],
        "GPT-6.1 Sol"
    );
}
