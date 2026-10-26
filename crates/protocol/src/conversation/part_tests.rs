use super::Part;
use serde_json::{Value, json};

fn call(summary: crate::tool::detail::Summary) -> Value {
    json!({"kind":"tool_call", "data":{
        "id":"call", "name":"read", "summary":summary,
        "presentation":"details", "grouping":"standalone"
    }})
}

#[test]
fn retains_bounded_summary_wire_shape() {
    let wire = call(crate::tool::detail::Summary {
        revision: "a".repeat(64),
        path: Some("source.rs".into()),
        has_details: true,
        ..Default::default()
    });
    let part: Part = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), wire);
    let Part::ToolCall { summary, .. } = part else {
        panic!("tool call expected")
    };
    assert_eq!(summary.path.as_deref(), Some("source.rs"));
    for field in ["arguments", "result", "display"] {
        assert!(wire["data"].get(field).is_none());
    }
}

#[test]
fn omits_absent_summary_fields() {
    let wire = call(Default::default());
    let part: Part = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(part).unwrap(), wire);
}
