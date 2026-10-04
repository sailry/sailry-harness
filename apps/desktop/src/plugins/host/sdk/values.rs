use gpui_shell::{HostError, HostValue};
use serde_json::Value;

pub(super) fn error_code(message: &str) -> &'static str {
    let message = message
        .strip_prefix("`sailry/sdk.")
        .and_then(|tail| tail.split_once("`: "))
        .filter(|(method, _)| {
            let mut characters = method.chars();
            characters.next().is_some_and(|character| {
                character.is_ascii_alphabetic() || matches!(character, '_' | '$')
            }) && characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
            })
        })
        .map_or(message, |(_, code)| code);
    match message {
        "configure" => "configure",
        "invalidResponse" => "invalidResponse",
        "unconfirmed" => "unconfirmed",
        "timedOut" => "timedOut",
        _ => "failed",
    }
}

pub(super) fn fault_code(message: &str) -> String {
    let message = message
        .strip_prefix('`')
        .and_then(|tail| tail.split_once("`: "))
        .filter(|(name, _)| name.starts_with("sailry/"))
        .map_or(message, |(_, message)| message);
    if let Ok(fault) = serde_json::from_str::<sailry_protocol::Fault>(message) {
        return serde_json::to_value(fault.code)
            .ok()
            .and_then(|code| code.as_str().map(str::to_owned))
            .unwrap_or_else(|| "failed".into());
    }
    if message == "unconfirmed" {
        "outcome_unknown".into()
    } else {
        "failed".into()
    }
}

pub(in crate::plugins) fn fault(fault: sailry_protocol::Fault) -> HostError {
    HostError::new(serde_json::to_string(&fault).unwrap_or(fault.message))
}

pub(in crate::plugins::host) fn revision(value: &str) -> Result<u64, HostError> {
    let revision: u64 = value
        .parse()
        .map_err(|_| HostError::new("invalid revision"))?;
    if revision.to_string() != value {
        return Err(HostError::new("invalid revision"));
    }
    Ok(revision)
}

pub(in crate::plugins::host) fn command(
    mut value: Value,
) -> Result<sailry_protocol::Command, HostError> {
    if let Some(revision) = value.pointer_mut("/data/package/settings_revision") {
        let exact = if let Some(text) = revision.as_str() {
            self::revision(text)?
        } else {
            revision
                .as_u64()
                .filter(|value| *value <= 9_007_199_254_740_991)
                .ok_or_else(|| HostError::new("package revision must be exact"))?
        };
        *revision = Value::from(exact);
    }
    serde_json::from_value(value).map_err(|_| HostError::new("invalid plugin command"))
}

pub(in crate::plugins) fn decode(value: &HostValue) -> Result<Value, HostError> {
    Ok(match value {
        HostValue::Null => Value::Null,
        HostValue::Bool(value) => Value::Bool(*value),
        HostValue::Number(value)
            if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 =>
        {
            Value::from(*value as i64)
        }
        HostValue::Number(value) => serde_json::Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| HostError::new("non-finite plugin number"))?,
        HostValue::Str(value) => Value::String(value.clone()),
        HostValue::Array(values) => {
            Value::Array(values.iter().map(decode).collect::<Result<_, _>>()?)
        }
        HostValue::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), decode(value)?)))
                .collect::<Result<_, HostError>>()?,
        ),
    })
}

pub(in crate::plugins) fn encode(value: Value) -> Result<HostValue, HostError> {
    Ok(match value {
        Value::Null => HostValue::Null,
        Value::Bool(value) => HostValue::Bool(value),
        Value::Number(value) => HostValue::Number(
            value
                .as_f64()
                .ok_or_else(|| HostError::new("invalid plugin number"))?,
        ),
        Value::String(value) => HostValue::Str(value),
        Value::Array(values) => {
            HostValue::Array(values.into_iter().map(encode).collect::<Result<_, _>>()?)
        }
        Value::Object(values) => HostValue::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, encode(value)?)))
                .collect::<Result<_, HostError>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    mod error_codes {
        use super::*;

        #[test]
        fn preserves_plain_codes() {
            for code in [
                "configure",
                "failed",
                "invalidResponse",
                "unconfirmed",
                "timedOut",
            ] {
                assert_eq!(error_code(code), code);
            }
        }

        #[test]
        fn recognizes_native_call_prefixes() {
            for (method, code) in [
                ("resolveModel", "configure"),
                ("modelChoice", "invalidResponse"),
                ("completeRequest", "unconfirmed"),
                ("prepareModel", "failed"),
                ("completeRequest", "timedOut"),
            ] {
                assert_eq!(error_code(&format!("`sailry/sdk.{method}`: {code}")), code);
            }
        }

        #[test]
        fn rejects_unrelated_messages() {
            for message in [
                "",
                "request capacity exhausted",
                "configure ",
                "prefix: configure",
                "`other.resolveModel`: configure",
                "`sailry/sdk-other.resolveModel`: configure",
                "`sailry/sdk.resolveModel` configure",
                "`sailry/sdk.resolveModel`:configure",
                "Error: `sailry/sdk.resolveModel`: configure",
                "`sailry/sdk.`: configure",
                "`sailry/sdk.resolve.Model`: configure",
                "`sailry/sdk.resolveModel `: configure",
                "`sailry/sdk.resolveModel`: unknown",
                "`sailry/sdk.resolveModel`: invalidResponse: configure",
                "`sailry/sdk.resolveModel`: `sailry/sdk.resolveModel`: configure",
            ] {
                assert_eq!(error_code(message), "failed", "{message}");
            }
        }
    }

    #[test]
    fn preserves_integer_commands_and_fractional_data() {
        assert_eq!(decode(&HostValue::Number(12.0)).unwrap().as_u64(), Some(12));
        assert_eq!(
            decode(&HostValue::Number(-12.0)).unwrap().as_i64(),
            Some(-12)
        );
        assert_eq!(decode(&HostValue::Number(1.5)).unwrap(), json!(1.5));
        assert!(decode(&HostValue::Number(f64::NAN)).is_err());
        assert!(decode(&HostValue::Number(f64::INFINITY)).is_err());
    }

    #[test]
    fn revisions_keep_all_bits() {
        assert_eq!(revision("9223372036854775807").unwrap(), i64::MAX as u64);
        for value in ["01", "-1", "+1", "1.0", "", "18446744073709551616"] {
            assert!(revision(value).is_err());
        }
    }
}

#[cfg(test)]
mod faults {
    use super::*;
    #[test]
    fn keeps_codes_through_host_prefixes() {
        let error = fault(sailry_protocol::Fault::new(
            sailry_protocol::ErrorCode::RevisionConflict,
            "directory changed",
        ))
        .to_string();
        assert_eq!(fault_code(&error), "revision_conflict");
        assert_eq!(
            fault_code(&format!("`sailry/sdk.listDirectory`: {error}")),
            "revision_conflict"
        );
        assert_eq!(
            fault_code("`sailry/documents.saveDocument`: unconfirmed"),
            "outcome_unknown"
        );
        assert_eq!(fault_code("`other/listDirectory`: unconfirmed"), "failed");
        assert_eq!(fault_code("directory changed"), "failed");
    }
}
