use super::Part;
use serde_json::{Value, json};

fn call(display: Option<Value>) -> Value {
    let mut wire = json!({"kind":"tool_call", "data":{
        "id":"call", "name":"read", "arguments":{"path":"source.rs"},
        "presentation":"details", "grouping":"standalone"
    }});
    if let Some(display) = display {
        wire["data"]["display"] = display;
    }
    wire
}

#[test]
fn retains_display_wire_shape() {
    let wire = call(Some(json!({
        "label":"Read", "locales":{"en":"Read"},
        "input":{"summary":{"path":"/path","code":true,"omit":[]},
                 "context":null,"target":null,"content":null}
    })));
    let part: Part = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), wire);
    let Part::ToolCall { display, .. } = part else {
        panic!("tool call expected")
    };
    assert!(display.unwrap().valid());
}

#[test]
fn omits_absent_display() {
    let wire = call(None);
    let part: Part = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(part).unwrap(), wire);
}
