use super::*;
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "missing_capabilities_are_disabled",
            (|scenario: &'static str| {
                let value =
                    json!({"id":"fixture","context":4096,"output":1024,"future_metadata":true});
                let model: Model = serde_json::from_value(value).unwrap();
                assert!(model.capabilities.is_none(), "{scenario}");
                assert!(!model.vision, "{scenario}");
                assert!(!model.tools, "{scenario}");
                assert!(!model.reasoning, "{scenario}");
                assert!(
                    serde_json::to_value(&model)
                        .unwrap()
                        .get("web_search")
                        .is_none(),
                    "{scenario}"
                );
                assert!(model.generates.is_empty(), "{scenario}");
                assert!(model.efforts.is_empty(), "{scenario}");
                assert!(!model.custom_efforts, "{scenario}");
                assert_eq!(model.default_effort, crate::Effort::Default, "{scenario}");
                assert_eq!(model.compaction_tokens, None, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "preserves_configured_compaction_threshold",
            (|scenario: &'static str| {
                let value = json!({
                    "id": "fixture",
                    "context": 1_050_000,
                    "output": 128_000,
                    "compaction_tokens": 200_000,
                });
                let model: Model = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(model.compaction_tokens, Some(200_000), "{scenario}");
                assert_eq!(
                    serde_json::to_value(model).unwrap()["compaction_tokens"],
                    value["compaction_tokens"],
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_native_capability_observations",
            (|scenario: &'static str| {
                let model: Model = serde_json::from_value(json!({
                    "id": "fixture",
                    "context": 4096,
                    "output": 1024,
                    "capabilities": {"vision": false, "tools": true},
                }))
                .unwrap();
                let native = model.capabilities.as_ref().unwrap();
                assert_eq!(native.vision, Some(false), "{scenario}");
                assert_eq!(native.tools, Some(true), "{scenario}");
                assert_eq!(native.reasoning, None, "{scenario}");
                assert_eq!(
                    serde_json::to_value(model).unwrap()["capabilities"]["vision"],
                    false,
                    "{scenario}"
                );
            }),
        ),
        (
            "identity_and_capacity_remain_required",
            (|scenario: &'static str| {
                let value = json!({"id":"fixture","context":4096,"output":1024});
                for field in ["id", "context", "output"] {
                    let mut missing = value.clone();
                    missing.as_object_mut().unwrap().remove(field);
                    assert!(
                        serde_json::from_value::<Model>(missing).is_err(),
                        "{scenario}"
                    );
                }
                for field in ["vision", "tools", "reasoning"] {
                    let mut malformed = value.clone();
                    malformed[field] = json!("true");
                    assert!(
                        serde_json::from_value::<Model>(malformed).is_err(),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "display_name_does_not_change_execution_identity",
            (|scenario: &'static str| {
                let model: Model = serde_json::from_value(json!({
                    "id": "gpt-6.1-sol",
                    "display_name": "GPT-6.1 Sol",
                    "context": 4096,
                    "output": 1024,
                }))
                .unwrap();
                assert_eq!(model.id, "gpt-6.1-sol", "{scenario}");
                assert_eq!(
                    model.display_name.as_deref(),
                    Some("GPT-6.1 Sol"),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::to_value(model).unwrap()["display_name"],
                    "GPT-6.1 Sol",
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
