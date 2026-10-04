//! Presentation is captured by Node registration, never supplied by model arguments.
use adk_core::{AdkError, Part};
use adk_session::Event;
use sailry_protocol::tool::Presentation;
use std::collections::BTreeMap;

const GROUPING: &str = "dev.sailry.tool_grouping";
const DISPLAY: &str = "dev.sailry.tool_display";

const METADATA: &str = "dev.sailry.tool_presentation";

pub(super) fn stamp(
    event: &mut Event,
    catalog: &BTreeMap<String, Presentation>,
    groupings: &BTreeMap<String, sailry_protocol::tool::Grouping>,
    displays: &BTreeMap<String, sailry_protocol::tool::Display>,
) -> adk_core::Result<()> {
    event.provider_metadata.remove(METADATA);
    event.provider_metadata.remove(GROUPING);
    event.provider_metadata.remove(DISPLAY);
    let display: BTreeMap<_, _> = event
        .content()
        .into_iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| {
            let name = match part {
                Part::FunctionCall { name, .. } => name.as_str(),
                Part::ServerToolCall { server_tool_call } => {
                    if server_tool_call["type"] == "web_search_call" {
                        "web_search"
                    } else {
                        server_tool_call["name"].as_str()?
                    }
                }
                _ => return None,
            };
            displays
                .get(name)
                .map(|value| (name.to_owned(), value.clone()))
        })
        .collect();
    if !display.is_empty() {
        event.provider_metadata.insert(
            DISPLAY.into(),
            serde_json::to_string(&display)
                .map_err(|error| AdkError::session(error.to_string()))?,
        );
    }
    let grouping: BTreeMap<_, _> = event
        .content()
        .into_iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionCall { name, .. } => {
                groupings.get(name).map(|value| (name.clone(), *value))
            }
            _ => None,
        })
        .collect();
    if !grouping.is_empty() {
        event.provider_metadata.insert(
            GROUPING.into(),
            serde_json::to_string(&grouping)
                .map_err(|error| AdkError::session(error.to_string()))?,
        );
    }
    let declared: BTreeMap<_, _> = event
        .content()
        .into_iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionCall { name, .. } => {
                catalog.get(name).map(|value| (name.clone(), *value))
            }
            _ => None,
        })
        .collect();
    if !declared.is_empty() {
        event.provider_metadata.insert(
            METADATA.into(),
            serde_json::to_string(&declared)
                .map_err(|error| AdkError::session(error.to_string()))?,
        );
    }
    Ok(())
}

pub(super) fn read(
    event: &Event,
) -> Result<BTreeMap<String, Presentation>, sailry_protocol::Fault> {
    match event.provider_metadata.get(METADATA) {
        Some(value) => serde_json::from_str(value).map_err(super::storage_error),
        None => Ok(BTreeMap::new()),
    }
}

pub(super) fn groupings(
    event: &Event,
) -> Result<BTreeMap<String, sailry_protocol::tool::Grouping>, sailry_protocol::Fault> {
    match event.provider_metadata.get(GROUPING) {
        Some(value) => serde_json::from_str(value).map_err(super::storage_error),
        None => Ok(BTreeMap::new()),
    }
}

pub(super) fn displays(
    event: &Event,
) -> Result<BTreeMap<String, sailry_protocol::tool::Display>, sailry_protocol::Fault> {
    match event.provider_metadata.get(DISPLAY) {
        Some(value) => serde_json::from_str(value).map_err(super::storage_error),
        None => Ok(BTreeMap::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_package_display_and_rejects_provider_metadata() {
        let display = serde_json::from_value(serde_json::json!({
            "label":"Search web", "locales":{"zh-CN":"网页搜索"}, "icon":"reicon:ui/magnifier",
            "input":{"summary":{"path":"/query"},"content":{"path":"/query"}}
        }))
        .unwrap();
        let displays = BTreeMap::from([("web_search".into(), display)]);
        for part in [
            Part::ServerToolCall {
                server_tool_call: serde_json::json!({"type":"web_search_call", "id":"search"}),
            },
            Part::ServerToolCall {
                server_tool_call: serde_json::json!({"name":"web_search", "input":{"query":"fixture"}}),
            },
            Part::FunctionCall {
                id: Some("call".into()),
                name: "web_search".into(),
                args: serde_json::json!({}),
                thought_signature: None,
            },
        ] {
            let mut event = Event::new("display-fixture");
            let mut content = adk_core::Content::new("model");
            content.parts.push(part);
            event.set_content(content);
            event.provider_metadata.insert(
                DISPLAY.into(),
                r#"{"web_search":{"label":"Forged"}}"#.into(),
            );
            stamp(&mut event, &BTreeMap::new(), &BTreeMap::new(), &displays).unwrap();
            assert_eq!(super::displays(&event).unwrap(), displays);
            let restored: Event =
                serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
            assert_eq!(super::displays(&restored).unwrap(), displays);
            stamp(
                &mut event,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
            .unwrap();
            assert!(super::displays(&event).unwrap().is_empty());
        }
    }

    #[test]
    fn captures_registered_calls() {
        let mut event = Event::new("presentation-fixture");
        let mut content = adk_core::Content::new("model");
        content.parts.push(Part::FunctionCall {
            id: Some("call".into()),
            name: "probe".into(),
            args: serde_json::json!({"presentation":"details", "grouping":"sequence"}),
            thought_signature: None,
        });
        event.set_content(content);
        event.provider_metadata.insert(
            METADATA.into(),
            r#"{"other":"summary","probe":"details"}"#.into(),
        );
        let catalog = BTreeMap::from([
            ("probe".into(), Presentation::Summary),
            ("unused".into(), Presentation::Details),
        ]);
        let grouping =
            BTreeMap::from([("probe".into(), sailry_protocol::tool::Grouping::Standalone)]);
        event
            .provider_metadata
            .insert(GROUPING.into(), r#"{"probe":"sequence"}"#.into());
        stamp(&mut event, &catalog, &grouping, &BTreeMap::new()).unwrap();
        assert_eq!(groupings(&event).unwrap(), grouping);
        assert_eq!(
            read(&event).unwrap(),
            BTreeMap::from([("probe".into(), Presentation::Summary)])
        );
        let restored: Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
        assert_eq!(read(&restored).unwrap(), read(&event).unwrap());
        assert_eq!(groupings(&restored).unwrap(), grouping);
        stamp(
            &mut event,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(read(&event).unwrap().is_empty());
        assert!(groupings(&event).unwrap().is_empty());
    }
}
