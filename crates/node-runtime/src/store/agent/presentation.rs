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
    aliases: &[sailry_protocol::tool::Alias],
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
            displays.get(name).map(|value| {
                let mut value = value.clone();
                value.aliases.clear();
                if value
                    .input
                    .as_ref()
                    .and_then(|input| input.summary.as_ref())
                    .is_some_and(|field| {
                        field.format == Some(sailry_protocol::tool::InputFormat::Command)
                    })
                {
                    value.aliases.extend_from_slice(aliases);
                }
                (name.to_owned(), value)
            })
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

/// Capture public child labels with the call, so later history pages need no live lookup.
pub(super) fn capture_names(
    db: &rusqlite::Connection,
    session: sailry_protocol::SessionId,
    event: &mut Event,
    previous: Option<&[u8]>,
) -> Result<(), sailry_protocol::Fault> {
    use rusqlite::OptionalExtension as _;
    use sailry_protocol::tool::InputFormat;
    let mut captured = displays(event)?;
    if !captured.values().any(|display| {
        display
            .input
            .as_ref()
            .and_then(|input| input.summary.as_ref())
            .is_some_and(|field| field.format == Some(InputFormat::Session))
    }) {
        return Ok(());
    }
    let previous = previous
        .map(|body| serde_json::from_slice::<Event>(body).map_err(super::storage_error))
        .transpose()?;
    let original = previous.as_ref().map(displays).transpose()?;
    for (name, display) in &mut captured {
        let Some(field) = display
            .input
            .as_mut()
            .and_then(|input| input.summary.as_mut())
            .filter(|field| field.format == Some(InputFormat::Session))
        else {
            continue;
        };
        field.names.clear();
        if let Some(original) = &original {
            field.names = original
                .get(name)
                .and_then(|display| display.input.as_ref())
                .and_then(|input| input.summary.as_ref())
                .filter(|old| old.format == field.format && old.path == field.path)
                .map(|old| old.names.clone())
                .unwrap_or_default();
            continue;
        }
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            let Part::FunctionCall {
                name: tool, args, ..
            } = part
            else {
                continue;
            };
            if tool != name {
                continue;
            }
            let Some(target) = field
                .read(args)
                .and_then(|value| value.parse::<sailry_protocol::SessionId>().ok())
            else {
                continue;
            };
            let title: Option<String> = db.query_row(
                "SELECT json_extract(body,'$.task.title') FROM agent_background WHERE session=?1 AND kind='agent' AND source=?2 ORDER BY sequence DESC,rowid DESC LIMIT 1",
                rusqlite::params![session.to_string(), target.to_string()], |row| row.get(0),
            ).optional().map_err(super::storage_error)?.flatten();
            if let Some(title) = title
                .filter(|title| !title.trim().is_empty() && !title.chars().any(char::is_control))
            {
                field.names.insert(target.to_string(), title);
            }
        }
    }
    event.provider_metadata.insert(
        DISPLAY.into(),
        serde_json::to_string(&captured).map_err(super::storage_error)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezes_owned_session_names() {
        use crate::store::agent::tests::fixture::Fixture;
        let mut fixture = Fixture::new();
        let turn = fixture.queued();
        let parent = super::super::runs::get(&fixture.database.connection, turn).unwrap();
        let mut event = Event::new("session-names");
        event.author = "assistant".into();
        let mut content = adk_core::Content::new("model");
        let mut expected = BTreeMap::new();
        for title in ["A", "B", "C"] {
            let mut child = parent.clone();
            child.turn = sailry_protocol::TurnId::new();
            child.session = sailry_protocol::SessionId::new();
            super::super::background::child_started(
                &fixture.database.connection,
                &parent,
                &child,
                title,
            )
            .unwrap();
            expected.insert(child.session.to_string(), title.to_owned());
            content.parts.push(Part::FunctionCall {
                id: Some(format!("wait-{title}")),
                name: "observe".into(),
                args: serde_json::json!({"session":child.session}),
                thought_signature: None,
            });
        }
        event.set_content(content);
        let display = serde_json::from_value(serde_json::json!({"label":"Observe", "input":{"summary":{"path":"/session","format":"session"}}})).unwrap();
        stamp(
            &mut event,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([("observe".into(), display)]),
            &[],
        )
        .unwrap();
        let original = event.clone();
        capture_names(
            &fixture.database.connection,
            fixture.session,
            &mut event,
            None,
        )
        .unwrap();
        let captured = displays(&event).unwrap();
        let display = &captured["observe"];
        assert!(display.valid());
        assert_eq!(
            display
                .input
                .as_ref()
                .unwrap()
                .summary
                .as_ref()
                .unwrap()
                .names,
            expected
        );
        let persisted = serde_json::to_vec(&event).unwrap();
        fixture
            .database
            .connection
            .execute(
                "UPDATE agent_background SET body=json_set(body,'$.task.title','Changed')",
                [],
            )
            .unwrap();
        let mut repeated = original.clone();
        capture_names(
            &fixture.database.connection,
            fixture.session,
            &mut repeated,
            Some(&persisted),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&repeated).unwrap(),
            serde_json::to_value(&event).unwrap()
        );
        let mut foreign = original;
        capture_names(
            &fixture.database.connection,
            sailry_protocol::SessionId::new(),
            &mut foreign,
            None,
        )
        .unwrap();
        assert!(
            displays(&foreign).unwrap()["observe"]
                .input
                .as_ref()
                .unwrap()
                .summary
                .as_ref()
                .unwrap()
                .names
                .is_empty()
        );
        assert_eq!(
            serde_json::to_value(repeated.content()).unwrap(),
            serde_json::to_value(event.content()).unwrap()
        );
    }

    #[test]
    fn captures_trusted_command_aliases() {
        use sailry_protocol::tool::{Alias, AliasKind};
        let mut event = Event::new("command-display");
        let mut content = adk_core::Content::new("model");
        let arguments =
            serde_json::json!({"command":"/node/home/python/bin/python /node/home/task.py"});
        content.parts.push(Part::FunctionCall {
            id: Some("call".into()),
            name: "probe".into(),
            args: arguments.clone(),
            thought_signature: None,
        });
        event.set_content(content);
        event.provider_metadata.insert(
            DISPLAY.into(),
            r#"{"probe":{"label":"Forged","aliases":[{"path":"/node","kind":"python"}]}}"#.into(),
        );
        let display = serde_json::from_value(serde_json::json!({"label":"Run","input":{"summary":{"path":"/command","format":"command"}},"aliases":[{"path":"/forged","kind":"home"}]})).unwrap();
        let aliases = vec![
            Alias {
                path: "/node/home".into(),
                kind: AliasKind::Home,
            },
            Alias {
                path: "/node/home/python/bin/python".into(),
                kind: AliasKind::Python,
            },
        ];
        stamp(
            &mut event,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::from([("probe".into(), display)]),
            &aliases,
        )
        .unwrap();
        let restored: Event = serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
        assert_eq!(
            super::displays(&restored).unwrap()["probe"].aliases,
            aliases
        );
        let Part::FunctionCall { args, .. } = &restored.content().unwrap().parts[0] else {
            panic!("call expected")
        };
        assert_eq!(args, &arguments);
    }

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
            stamp(
                &mut event,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &displays,
                &[],
            )
            .unwrap();
            assert_eq!(super::displays(&event).unwrap(), displays);
            let restored: Event =
                serde_json::from_slice(&serde_json::to_vec(&event).unwrap()).unwrap();
            assert_eq!(super::displays(&restored).unwrap(), displays);
            stamp(
                &mut event,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &[],
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
        stamp(&mut event, &catalog, &grouping, &BTreeMap::new(), &[]).unwrap();
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
            &[],
        )
        .unwrap();
        assert!(read(&event).unwrap().is_empty());
        assert!(groupings(&event).unwrap().is_empty());
    }
}
