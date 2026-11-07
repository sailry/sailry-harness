//! Declared tool media lives at the result root or inside a typed Node output.
use sailry_protocol::conversation::images::Reference;
use serde_json::Value;

/// Node outputs whose data object may declare `inline_data` media.
const KINDS: [&str; 4] = ["computer", "browser", "external_browser", "file_image"];

pub(crate) fn enveloped(value: &Value) -> bool {
    value["kind"]
        .as_str()
        .is_some_and(|kind| KINDS.contains(&kind))
        && value["data"].is_object()
}

/// Returned images get references the model can use in Markdown replies.
pub(crate) fn refer(value: &mut Value, call: &str) {
    if !enveloped(value) {
        return;
    }
    let data = &mut value["data"];
    let count = data["inline_data"].as_array().map_or(0, Vec::len);
    if count == 0 {
        return;
    }
    data["images"] = (0..count)
        .map(|index| {
            Reference {
                call: call.into(),
                index,
            }
            .to_string()
        })
        .collect::<Vec<_>>()
        .into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognizes_media_envelopes() {
        for kind in KINDS {
            assert!(enveloped(&json!({"kind":kind,"data":{}})));
            assert!(!enveloped(&json!({"kind":kind,"data":"text"})));
        }
        for value in [
            json!({"kind":"plugin_value","data":{"inline_data":[]}}),
            json!({"kind":"file_lines","data":{"inline_data":[]}}),
            json!({"inline_data":[]}),
            json!(null),
        ] {
            assert!(!enveloped(&value));
        }
    }

    #[test]
    fn refers_to_each_inline_image() {
        let mut value = json!({"kind":"browser","data":{"tab":1,"inline_data":[{},{}]}});
        refer(&mut value, "call 1");
        assert_eq!(
            value["data"]["images"],
            json!(["sailry-image:call%201/0", "sailry-image:call%201/1"])
        );
        let mut value = json!({"kind":"file_image","data":{"path":"a.png","inline_data":[{}]}});
        refer(&mut value, "read");
        assert_eq!(value["data"]["images"], json!(["sailry-image:read/0"]));
        for original in [
            json!({"kind":"browser","data":{"tab":1}}),
            json!({"kind":"browser","data":{"inline_data":[]}}),
            json!({"kind":"plugin_value","data":{"inline_data":[{}]}}),
        ] {
            let mut value = original.clone();
            refer(&mut value, "call");
            assert_eq!(value, original);
        }
    }
}
