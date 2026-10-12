//! Only an admitted package handler can create a canonical controller receipt.
use sailry_protocol::{
    NodeId, RequestId,
    plugin::{self, card},
    tool,
};
use serde_json::Value;

pub(crate) fn identity(
    turn: sailry_protocol::TurnId,
    package: &plugin::Reference,
    tool: &str,
    call: &str,
) -> Result<RequestId, sailry_protocol::Fault> {
    let encoded = serde_json::to_vec(&(turn, package, tool, call))
        .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin tool identity"))?;
    let mut identity = blake3::Hasher::new_derive_key("Sailry plugin tool call v1");
    identity.update(&encoded);
    let mut bytes: [u8; 16] = identity.finalize().as_bytes()[..16]
        .try_into()
        .expect("fixed digest size");
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    RequestId::try_from(uuid::Uuid::from_bytes(bytes))
        .map_err(|_| sailry_protocol::Fault::invalid("invalid plugin tool identity"))
}

pub(crate) fn props(declaration: &card::Declaration, value: &Value) -> bool {
    declaration.accepts(value)
}

pub(crate) fn stamp(
    result: &mut Value,
    node: NodeId,
    context: &plugin::Context,
    package: &plugin::Info,
    call: RequestId,
) {
    let Some(raw) = result.get_mut("sailry_content") else {
        return;
    };
    let Some(blocks) = raw.get_mut("blocks").and_then(Value::as_array_mut) else {
        return;
    };
    let extension = package.extension.as_ref();
    for (index, block) in blocks.iter_mut().enumerate() {
        if block["kind"] != "card" {
            continue;
        }
        let draft = serde_json::from_value::<card::Draft>(
            block
                .clone()
                .as_object()
                .map(|object| {
                    let mut object = object.clone();
                    object.remove("kind");
                    Value::Object(object)
                })
                .unwrap_or(Value::Null),
        );
        let summary = draft.ok().and_then(|draft| {
            let declaration = extension?
                .cards
                .iter()
                .find(|entry| entry.key == draft.key)?;
            if !props(declaration, &draft.props)
                || !draft.fallback.valid()
                || index >= card::MAX_CARDS
            {
                return None;
            }
            Some(card::Summary {
                receipt: card::Receipt {
                    session: context.session?,
                    turn: context.turn?,
                    call,
                    block: index as u8,
                },
                node,
                worktree: context.worktree?,
                package: context.package.clone(),
                key: draft.key,
                props: draft.props,
                fallback: draft.fallback,
            })
        });
        let Some(summary) = summary.filter(card::Summary::valid) else {
            result
                .as_object_mut()
                .map(|object| object.remove("sailry_content"));
            return;
        };
        *block = serde_json::to_value(tool::Block::Card(summary))
            .expect("card metadata is serializable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (plugin::Info, plugin::Context) {
        let package: plugin::Info = serde_json::from_value(json!({
            "summary":{"name":"example","revision":1,"digest":"a".repeat(64),"settings_revision":0,"enabled":true},
            "extension":{"api_version":"v1","actions":[],"desktop":{"resources":["dev.sailry.platform/card.js"]},
                "cards":[{"key":"connection","entry":"dev.sailry.platform/card.js","props_schema":{"type":"object","additionalProperties":false,"properties":{"configured":{"type":"boolean"}},"required":["configured"]}}]}
        })).unwrap();
        let context = plugin::Context {
            invocation: None,
            turn: Some(sailry_protocol::TurnId::new()),
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(sailry_protocol::WorktreeId::new()),
            session: Some(sailry_protocol::SessionId::new()),
        };
        (package, context)
    }

    fn result() -> Value {
        json!({"isError":true,"error":{"code":"not_configured","message":"Connect"},
            "sailry_content":{"version":1,"blocks":[{"kind":"card","key":"connection","props":{"configured":false},"fallback":{"label":"Connect"}}]}})
    }

    #[test]
    fn stamps_only_the_admitted_ownership() {
        let (package, context) = fixture();
        let mut result = result();
        let original = result["error"].clone();
        let call = identity(context.turn.unwrap(), &context.package, "tool", "call").unwrap();
        stamp(&mut result, NodeId([1; 32]), &context, &package, call);
        let content = tool::Presentation::Content.content(&result).unwrap();
        let tool::Block::Card(card) = &content.blocks[0] else {
            panic!("card expected")
        };
        assert_eq!(card.receipt.call, call);
        assert_eq!(card.receipt.turn, context.turn.unwrap());
        assert_eq!(card.package, context.package);
        assert_eq!(result["error"], original);
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn discards_forged_presentation_not_original_results() {
        let (package, context) = fixture();
        for change in 0..4 {
            let mut result = result();
            match change {
                0 => {
                    result["sailry_content"]["blocks"][0]["receipt"] =
                        json!({"session":context.session})
                }
                1 => result["sailry_content"]["blocks"][0]["key"] = json!("unknown"),
                2 => result["sailry_content"]["blocks"][0]["props"]["token"] = json!("private"),
                3 => result["sailry_content"]["blocks"][0]["props"]["configured"] = json!("false"),
                _ => unreachable!(),
            }
            let original = result["error"].clone();
            stamp(
                &mut result,
                NodeId([1; 32]),
                &context,
                &package,
                RequestId::new(),
            );
            assert!(result.get("sailry_content").is_none());
            assert_eq!(result["error"], original);
            assert_eq!(result["isError"], true);
        }
    }
}
