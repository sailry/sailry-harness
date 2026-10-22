//! Preserve native diagnostics while presenting JSON objects as readable fields.
use serde_json::Value;

pub(super) fn display(message: &str) -> String {
    let (heading, body) = if message.starts_with("Agent execution failed (") {
        message.split_once("\n\n").unwrap_or(("", message))
    } else {
        ("", message)
    };
    let Ok(Value::Object(values)) = serde_json::from_str::<Value>(body) else {
        return message.to_owned();
    };
    if values.is_empty() {
        return message.to_owned();
    }
    let mut lines = Vec::new();
    for (key, value) in values {
        fields(&path("", &key), &value, &mut lines);
    }
    let body = lines.join("\n");
    if heading.is_empty() {
        body
    } else {
        format!("{heading}\n\n{body}")
    }
}

fn path(parent: &str, key: &str) -> String {
    if key.is_empty()
        || !key
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
    {
        format!("{parent}[{}]", Value::String(key.to_owned()))
    } else if parent.is_empty() {
        key.to_owned()
    } else {
        format!("{parent}.{key}")
    }
}

fn fields(key: &str, value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::Object(values) if !values.is_empty() => {
            for (name, value) in values {
                fields(&path(key, name), value, lines);
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for (index, value) in values.iter().enumerate() {
                fields(&format!("{key}[{index}]"), value, lines);
            }
        }
        Value::String(value) => lines.push(format!("{key}: {value}")),
        value => lines.push(format!("{key}: {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_nested_fields_without_dropping_values() {
        let output = display(
            r#"{"error":{"code":503,"message":"Temporarily unavailable","retry":true},"details":[null,{},[]],"request.id":"abc"}"#,
        );
        for line in [
            "error.code: 503",
            "error.message: Temporarily unavailable",
            "error.retry: true",
            "details[0]: null",
            "details[1]: {}",
            "details[2]: []",
            "[\"request.id\"]: abc",
        ] {
            assert!(
                output.lines().any(|actual| actual == line),
                "missing {line}"
            );
        }
        assert_eq!(output.lines().count(), 7);
    }

    #[test]
    fn preserves_classification_heading() {
        assert_eq!(
            display(
                "Agent execution failed (model.api_error); HTTP 503; unavailable\n\n{\"request_id\":\"abc\"}"
            ),
            "Agent execution failed (model.api_error); HTTP 503; unavailable\n\nrequest_id: abc"
        );
    }

    #[test]
    fn preserves_other_native_messages() {
        for message in [
            "Network unavailable",
            "{invalid JSON",
            "{}",
            "[1,2]",
            "null",
            "Provider detail\n\n{\"code\":503}",
            "Agent execution failed (model.api_error)\n\nnot JSON",
        ] {
            assert_eq!(display(message), message);
        }
    }
}
