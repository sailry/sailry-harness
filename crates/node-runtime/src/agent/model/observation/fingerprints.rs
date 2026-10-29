//! Opt-in adapted JSON fingerprints; raw input and protected configuration are discarded.
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    kind: &'static str,
    index: u64,
    digest: blake3::Hash,
    bytes: u64,
}

fn collect(body: &Value) -> Vec<Fingerprint> {
    let Some(object) = body.as_object() else {
        return Vec::new();
    };
    let mut fingerprints = Vec::new();
    let mut append = |kind, index, value: &Value| {
        let encoded = value.to_string();
        fingerprints.push(Fingerprint {
            kind,
            index,
            digest: blake3::hash(encoded.as_bytes()),
            bytes: encoded.len() as u64,
        });
    };
    for key in ["instructions", "system", "systemInstruction", "tools"] {
        if let Some(value) = object.get(key) {
            append(key, 0, value);
        }
    }
    for key in ["input", "messages", "contents"] {
        match object.get(key) {
            Some(Value::Array(messages)) => {
                for (index, message) in messages.iter().enumerate() {
                    append(key, index as u64, message);
                }
            }
            Some(value) => append(key, 0, value),
            None => {}
        }
    }
    let settings = object
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "instructions"
                    | "system"
                    | "systemInstruction"
                    | "tools"
                    | "input"
                    | "messages"
                    | "contents"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    append("settings", 0, &Value::Object(settings));
    fingerprints
}

pub(super) fn record(body: &Value, identity: &crate::logging::Context, attempt: u64) {
    let session = identity
        .session
        .map(|id| id.to_string())
        .unwrap_or_default();
    let turn = identity.turn.map(|id| id.to_string()).unwrap_or_default();
    let call = identity.call.map(|id| id.to_string()).unwrap_or_default();
    for fingerprint in collect(body) {
        tracing::debug!(
            target: "sailry",
            area = "model",
            phase = "selected",
            operation = "request_fingerprint",
            session = session.as_str(),
            turn = turn.as_str(),
            call = call.as_str(),
            attempt,
            source_kind = fingerprint.kind,
            sequence = fingerprint.index,
            digest = fingerprint.digest.to_hex().as_str(),
            bytes = fingerprint.bytes,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_unchanged_message_prefix() {
        let first = json!({"instructions":"fixed","input":[
            {"role":"user","content":"task"},
            {"role":"assistant","content":"answer"},
        ]});
        let mut second = first.clone();
        second["input"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":"next"}));
        let messages = |body: &Value| {
            collect(body)
                .into_iter()
                .filter(|item| item.kind == "input")
                .collect::<Vec<_>>()
        };
        assert_eq!(messages(&first), messages(&second)[..2]);
        second["input"][1]["content"] = json!("changed");
        assert_eq!(messages(&first)[0], messages(&second)[0]);
        assert_ne!(messages(&first)[1], messages(&second)[1]);
        assert_eq!(messages(&second)[1].index, 1);
    }

    #[test]
    fn detects_tool_order_and_settings() {
        let first = json!({"tools":[{"name":"zeta"},{"name":"alpha"}],"store":false,"prompt_cache_key":"session"});
        let mut second = first.clone();
        second["tools"].as_array_mut().unwrap().reverse();
        assert_ne!(collect(&first)[0], collect(&second)[0]);
        assert_eq!(collect(&first)[1], collect(&second)[1]);
        second["store"] = json!(true);
        assert_ne!(collect(&first)[1], collect(&second)[1]);
    }

    #[test]
    fn excludes_raw_payloads() {
        let body = json!({"system":"PRIVATE_INSTRUCTIONS", "messages":[
            {"role":"tool","content":"PRIVATE_OUTPUT"}
        ], "tools":[{"description":"PRIVATE_SCHEMA"}], "metadata":"PRIVATE_METADATA"});
        let fingerprints = collect(&body);
        assert_eq!(fingerprints.len(), 4);
        assert!(!format!("{fingerprints:?}").contains("PRIVATE_"));
        assert!(collect(&Value::Null).is_empty());
    }
}
