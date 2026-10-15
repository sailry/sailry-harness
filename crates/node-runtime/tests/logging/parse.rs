//! Test reader for the public human-readable diagnostic lines.
use serde_json::{Map, Value};

pub fn line(line: &str) -> Value {
    let mut parts = line.splitn(4, ' ');
    let timestamp = parts.next().expect("timestamp");
    chrono::DateTime::parse_from_rfc3339(timestamp).expect("RFC3339 timestamp");
    let level = parts.next().expect("level");
    assert!(matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG"));
    let application = parts.next().expect("application");
    assert!(matches!(application, "desktop" | "host"));
    let mut record = Map::from_iter([
        ("timestamp".into(), timestamp.into()),
        ("level".into(), level.into()),
        ("application".into(), application.into()),
    ]);
    let mut rest = parts.next().unwrap_or("");
    while !rest.is_empty() {
        let (key, value) = rest.split_once('=').expect("key=value field");
        assert!(!key.is_empty() && !key.chars().any(char::is_whitespace));
        let (value, consumed) = if value.starts_with('"') {
            let mut parser = serde_json::Deserializer::from_str(value).into_iter::<Value>();
            let parsed = parser
                .next()
                .expect("quoted value")
                .expect("valid JSON string");
            assert!(parsed.is_string());
            (parsed, parser.byte_offset())
        } else {
            let end = value.find(' ').unwrap_or(value.len());
            let token = &value[..end];
            assert!(!token.is_empty());
            (
                serde_json::from_str(token).unwrap_or_else(|_| token.into()),
                end,
            )
        };
        assert!(
            record.insert(key.into(), value).is_none(),
            "duplicate field"
        );
        rest = &rest[key.len() + 1 + consumed..];
        if !rest.is_empty() {
            rest = rest.strip_prefix(' ').expect("field separator");
            assert!(!rest.is_empty(), "trailing separator");
        }
    }
    Value::Object(record)
}
