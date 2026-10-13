use sailry_protocol::plugin::{self, Extension};

pub(super) fn valid(extension: &Extension) -> bool {
    extension.cards.len() <= plugin::card::MAX_CARDS
        && extension.cards.iter().enumerate().all(|(index, card)| {
            card.valid(extension)
                && !extension.cards[..index]
                    .iter()
                    .any(|other| other.key == card.key)
                && jsonschema::draft202012::options()
                    .build(&card.props_schema)
                    .is_ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extension(card: serde_json::Value) -> Extension {
        serde_json::from_value(json!({
            "api_version":"v1",
            "actions":[],
            "host":{
                "entry":"dev.sailry.platform/host.js",
                "resources":["dev.sailry.platform/host.js"],
                "handlers":["connection"]
            },
            "desktop":{"resources":["dev.sailry.platform/card.js"]},
            "tools":[{
                "name":"connection",
                "description":"Read connection",
                "presentation":"content",
                "handler":{
                    "name":"connection",
                    "read_only":true,
                    "parameters":{"type":"object","properties":{},"additionalProperties":false}
                }
            }],
            "cards":[card]
        }))
        .unwrap()
    }

    fn card() -> serde_json::Value {
        json!({
            "key":"connection",
            "entry":"dev.sailry.platform/card.js",
            "tool":"connection",
            "mcp_server":"input",
            "props_schema":{
                "type":"object",
                "additionalProperties":false,
                "properties":{"configured":{"type":"boolean"}},
                "required":["configured"]
            },
            "capabilities":{"browser":[{
                "name":"login",
                "origins":["https://example.test"],
                "source":{"kind":"mcp_authorization"},
                "modes":["external"]
            }]}
        })
    }

    #[test]
    fn accepts_optional_connection_metadata() {
        let declared = extension(card());
        assert!(valid(&declared));
        let mut legacy = card();
        legacy.as_object_mut().unwrap().remove("tool");
        assert!(valid(&extension(legacy)));
    }

    #[test]
    fn requires_a_read_only_content_handler() {
        for mutation in 0..4 {
            let mut declared = extension(card());
            match mutation {
                0 => declared.cards[0].tool = Some("missing".into()),
                1 => declared.tools[0].handler.as_mut().unwrap().read_only = false,
                2 => declared.tools[0].presentation = sailry_protocol::tool::Presentation::Details,
                3 => {
                    declared.tools[0].handler.as_mut().unwrap().operation =
                        Some(sailry_protocol::tool::Operation::SetValue)
                }
                _ => unreachable!(),
            }
            assert!(!valid(&declared));
        }
    }

    #[test]
    fn settings_requires_a_declared_surface() {
        let mut declared = extension(card());
        declared.cards[0].capabilities.settings = true;
        assert!(!valid(&declared));
        declared.settings_schema = Some("dev.sailry.platform/settings.json".into());
        assert!(valid(&declared));
    }
}
