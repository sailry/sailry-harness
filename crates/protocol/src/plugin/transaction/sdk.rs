//! Shared value adaptation for desktop and headless JavaScript SDKs.
use super::*;
use crate::{ErrorCode, Fault, Output};
use serde_json::{Value, json};

pub fn decode(mut value: Value) -> Result<Vec<Operation>, Fault> {
    let operations = value.as_array_mut().ok_or_else(invalid)?;
    if operations.is_empty() || operations.len() > MAX_OPERATIONS {
        return Err(invalid());
    }
    for operation in operations {
        let pointer = match operation["kind"].as_str() {
            Some(
                "write"
                | "index"
                | "remove"
                | "conversation_write"
                | "conversation_remove"
                | "submit"
                | "continue",
            ) => Some("/data/expected_revision"),
            Some("dispatch") => match operation["data"]["kind"].as_str() {
                Some("save_handler" | "save_schedule") => Some("/data/data/revision"),
                Some("remove_handler" | "remove_schedule") => Some("/data/data/expected_revision"),
                _ => None,
            },
            _ => None,
        };
        if let Some(pointer) = pointer {
            let revision = operation.pointer_mut(pointer).ok_or_else(invalid)?;
            let exact = if let Some(text) = revision.as_str() {
                let number = text.parse::<u64>().map_err(|_| invalid())?;
                if text != number.to_string() {
                    return Err(invalid());
                }
                number
            } else {
                revision
                    .as_u64()
                    .filter(|number| *number <= 9_007_199_254_740_991)
                    .ok_or_else(invalid)?
            };
            *revision = json!(exact);
        }
    }
    let operations: Vec<Operation> = serde_json::from_value(value).map_err(|_| invalid())?;
    validate(&operations)?;
    Ok(operations)
}

/// Writable revisions cross the JavaScript boundary as decimal strings.
pub fn output(output: Output) -> Value {
    let mut value = serde_json::to_value(&output).expect("protocol values are serializable");
    match output {
        Output::PluginTransaction(outputs) => {
            value["data"] = Value::Array(outputs.into_iter().map(self::output).collect());
        }
        Output::PluginValue(entry) => value["data"]["revision"] = json!(entry.revision.to_string()),
        Output::PluginConversationValue(entry) => {
            value["data"]["revision"] = json!(entry.revision.to_string())
        }
        Output::Session(session) => value["data"]["revision"] = json!(session.revision.to_string()),
        Output::PluginSettings(settings) => {
            value["data"]["package"]["settings_revision"] =
                json!(settings.package.settings_revision.to_string());
        }
        Output::PluginSearch(page) => {
            for (value, entry) in value["data"]["entries"]
                .as_array_mut()
                .expect("typed entries")
                .iter_mut()
                .zip(page.entries)
            {
                value["revision"] = json!(entry.revision.to_string());
            }
        }
        Output::Dispatch(dispatch::Output::Handler(_) | dispatch::Output::Schedule(_)) => {
            let record = &mut value["data"]["data"];
            record["revision"] = json!(
                record["revision"]
                    .as_u64()
                    .expect("typed revision")
                    .to_string()
            );
        }
        _ => {}
    }
    value
}

fn invalid() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "invalid plugin transaction or revision",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisions_remain_exact() {
        let operations = decode(json!([
            {"kind":"write","data":{"key":"item","value":{},"expected_revision":"9007199254740993"}},
            {"kind":"dispatch","data":{"kind":"save_schedule","data":{"id":crate::ScheduleId::new(),"revision":"9007199254740993","enabled":true,"topic":"tasks","payload":null,"timing":{"kind":"once","data":{"at_ms":1}},"next_ms":null}}}
        ])).unwrap();
        assert!(matches!(
            operations[0],
            Operation::Write {
                expected_revision: 9007199254740993,
                ..
            }
        ));
        assert!(
            matches!(&operations[1], Operation::Dispatch(dispatch::Command::SaveSchedule(schedule)) if schedule.revision == 9007199254740993)
        );
        for revision in [
            json!(9007199254740992u64),
            json!("01"),
            json!("-1"),
            json!("1.0"),
            json!("18446744073709551616"),
            json!(1.5),
        ] {
            assert!(
                decode(
                    json!([{"kind":"remove","data":{"key":"item","expected_revision":revision}}])
                )
                .is_err()
            );
        }
        let output = output(Output::PluginTransaction(vec![
            Output::PluginValue(super::super::super::storage::Entry {
                key: "item".into(),
                revision: 9007199254740993,
                value: json!(true),
                present: true,
            }),
            Output::Dispatch(dispatch::Output::Schedule(dispatch::Schedule {
                id: crate::ScheduleId::new(),
                revision: 9007199254740993,
                enabled: true,
                topic: "tasks".into(),
                payload: Value::Null,
                timing: dispatch::Timing::Once { at_ms: 1 },
                next_ms: None,
            })),
        ]));
        assert_eq!(output["data"][0]["data"]["revision"], "9007199254740993");
        assert_eq!(
            output["data"][1]["data"]["data"]["revision"],
            "9007199254740993"
        );
    }

    #[test]
    fn rejects_nontransactional_operations() {
        for operations in [
            json!([]),
            json!([{"kind":"call_plugin","data":{"handler":"run","input":null}}]),
            json!([{"kind":"dispatch","data":{"kind":"list_handlers"}}]),
        ] {
            assert!(decode(operations).is_err());
        }
    }

    #[test]
    fn indexed_values_share_the_exact_revision_boundary() {
        let operations=decode(json!([{"kind":"index","data":{"key":"item","value":{},"index":{"fields":["title","body"],"tags":["opaque"],"order":1},"expected_revision":"9007199254740993"}}])).unwrap();
        assert!(matches!(
            operations[0],
            Operation::Index {
                expected_revision: 9007199254740993,
                ..
            }
        ));
        let page = output(Output::PluginSearch(
            super::super::super::storage::SearchPage {
                entries: vec![super::super::super::storage::Entry {
                    key: "item".into(),
                    revision: 9007199254740993,
                    value: json!({}),
                    present: true,
                }],
                next: None,
                now_ms: 1,
            },
        ));
        assert_eq!(page["data"]["entries"][0]["revision"], "9007199254740993");
    }

    #[test]
    fn conversation_revisions() {
        let operations = decode(json!([
            {"kind":"conversation_write","data":{"key":"state","value":{"phase":"initial"},"expected_revision":"9007199254740993"}},
            {"kind":"conversation_remove","data":{"key":"state","expected_revision":"9007199254740994"}}
        ])).unwrap();
        assert!(matches!(
            operations[0],
            Operation::ConversationWrite {
                expected_revision: 9007199254740993,
                ..
            }
        ));
        assert!(matches!(
            operations[1],
            Operation::ConversationRemove {
                expected_revision: 9007199254740994,
                ..
            }
        ));
        let value = output(Output::PluginConversationValue(
            super::super::super::storage::ConversationEntry {
                key: "state".into(),
                revision: 9007199254740993,
                value: Value::Null,
                present: false,
                restored: true,
            },
        ));
        assert_eq!(value["data"]["revision"], "9007199254740993");
        assert_eq!(value["data"]["restored"], true);
        assert_eq!(value["data"]["present"], false);
    }

    #[test]
    fn continuation_scopes() {
        let session = crate::SessionId::new();
        let mut operation = json!({"kind":"continue","data":{
            "session":session,"after":null,"message":{"text":"Continue","references":[],"attachments":[]},
            "key":"state","expected_revision":"1"
        }});
        assert!(decode(json!([operation.clone()])).is_err());
        operation["data"]["scope"] = json!("conversation");
        let operations = decode(json!([operation])).unwrap();
        assert!(matches!(
            operations[0],
            Operation::Continue {
                scope: super::super::super::storage::Scope::Conversation,
                ..
            }
        ));
    }
}
