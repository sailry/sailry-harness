//! Shared JavaScript adaptation for session selection and association.
use crate::Fault;
use serde_json::{Value, json};

/// Preserve exact catalog revisions before decoding a protocol command.
pub fn command(value: &mut Value) -> Result<(), Fault> {
    match value["kind"].as_str() {
        Some("start_session" | "set_session_team") => {
            if let Some(value) = value
                .pointer_mut("/data/team/revision")
                .filter(|value| !value.is_null())
            {
                revision(value)?;
            }
        }
        Some("dispatch") => {
            if let Some(action) = value.pointer_mut("/data/action") {
                handler(action)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A saved callback uses the same selection encoding as a direct request.
pub fn handler(value: &mut Value) -> Result<(), Fault> {
    if value["kind"] == "save_handler"
        && let Some(command_value) = value.pointer_mut("/data/callback/command")
    {
        command(command_value)?;
    }
    Ok(())
}

pub fn association(value: &mut Value, namespace: &str) -> Result<(), Fault> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| Fault::invalid("invalid session association"))?;
    if object
        .get("namespace")
        .is_some_and(|value| value.as_str() != Some(namespace))
    {
        return Err(Fault::new(
            crate::ErrorCode::PermissionDenied,
            "session association belongs to another plugin",
        ));
    }
    object.insert("namespace".into(), json!(namespace));
    Ok(())
}

fn revision(value: &mut Value) -> Result<(), Fault> {
    let exact = if let Some(text) = value.as_str() {
        let parsed = text
            .parse::<u64>()
            .map_err(|_| Fault::invalid("invalid team revision"))?;
        if parsed.to_string() != text {
            return Err(Fault::invalid("invalid team revision"));
        }
        parsed
    } else {
        value
            .as_u64()
            .filter(|value| *value <= 9_007_199_254_740_991)
            .ok_or_else(|| Fault::invalid("team revision must be exact"))?
    };
    *value = json!(exact);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "keeps_exact_team_revisions",
                (|scenario: &'static str| {
                    for kind in ["start_session", "set_session_team"] {
                        let mut value = json!({"kind":kind,"data":{"team":{"id":"fixture","revision":"9007199254740993"}}});
                        command(&mut value).unwrap();
                        assert_eq!(
                            value["data"]["team"]["revision"],
                            json!(9_007_199_254_740_993_u64),
                            "{scenario}"
                        );
                        for revision in [
                            json!(9_007_199_254_740_993_u64),
                            json!("01"),
                            json!(-1),
                            json!(1.5),
                        ] {
                            value["data"]["team"]["revision"] = revision;
                            assert!(command(&mut value).is_err(), "{scenario}");
                        }
                        value["data"]["team"]["revision"] = Value::Null;
                        command(&mut value).unwrap();
                        assert!(value["data"]["team"]["revision"].is_null(), "{scenario}");
                    }
                }) as fn(&'static str),
            ),
            (
                "normalizes_callback_selections",
                (|scenario: &'static str| {
                    let mut value = json!({"kind":"dispatch","data":{"action":{"kind":"save_handler","data":{"callback":{"command":{"kind":"start_session","data":{"team":{"revision":"9007199254740993"}}}}}}}});
                    command(&mut value).unwrap();
                    assert_eq!(
                        value.pointer("/data/action/data/callback/command/data/team/revision"),
                        Some(&json!(9_007_199_254_740_993_u64)),
                        "{scenario}"
                    );
                }),
            ),
            (
                "retains_association_ownership",
                (|scenario: &'static str| {
                    let mut value = json!({"kind":"task","id":"daily"});
                    association(&mut value, "fixture").unwrap();
                    assert_eq!(
                        value,
                        json!({"namespace":"fixture","kind":"task","id":"daily"}),
                        "{scenario}"
                    );
                    association(&mut value, "fixture").unwrap();
                    assert_eq!(
                        association(&mut value, "other").unwrap_err().code,
                        crate::ErrorCode::PermissionDenied,
                        "{scenario}"
                    );
                    assert!(
                        association(&mut Value::Null, "fixture").is_err(),
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
