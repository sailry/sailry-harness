//! Manifest validation adapted from harbor-agent-plugins 727ce0a (Apache-2.0).
//! Agent Plugins 1.0.0 defines the portable fields and failure boundaries.
use sailry_protocol::{
    Fault,
    plugin::{Extension, Issue, IssueKind},
};
use serde_json::{Map, Value};

use super::invalid;

pub(super) const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const FIELDS: &[&str] = &[
    "$schema",
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "extensions",
];

pub(super) struct Manifest {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub issues: Vec<Issue>,
    pub extension: Option<Extension>,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Manifest, Fault> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| invalid("invalid plugin manifest JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("plugin manifest must be an object"))?;
    if string(object, "$schema")? != Some(SCHEMA) {
        return Err(invalid("unsupported Agent Plugins manifest schema"));
    }
    let name = string(object, "name")?.ok_or_else(|| invalid("plugin name is required"))?;
    validate_name(name)?;
    for field in ["homepage", "repository", "license"] {
        string(object, field)?;
    }
    if let Some(author) = object.get("author") {
        let author = author
            .as_object()
            .ok_or_else(|| invalid("plugin author must be an object"))?;
        for key in author.keys() {
            if !["name", "email", "url"].contains(&key.as_str()) {
                return Err(invalid("plugin author contains an unknown field"));
            }
            string(author, key)?;
        }
    }
    if let Some(keywords) = object.get("keywords") {
        let keywords = keywords
            .as_array()
            .ok_or_else(|| invalid("plugin keywords must be an array"))?;
        if keywords.iter().any(|keyword| !keyword.is_string()) {
            return Err(invalid("plugin keywords must contain strings"));
        }
    }
    let mut issues: Vec<_> = object
        .keys()
        .filter(|key| !FIELDS.contains(&key.as_str()))
        .map(|key| Issue {
            path: format!("plugin.json.{key}"),
            kind: IssueKind::IgnoredManifestField,
        })
        .collect();
    let mut extension = None;
    if let Some(extensions) = object.get("extensions") {
        match extensions.as_object() {
            None => issues.push(Issue {
                path: "plugin.json.extensions".into(),
                kind: IssueKind::InvalidExtensions,
            }),
            Some(extensions) if extensions.contains_key("dev.sailry.platform") => {
                let value = &extensions["dev.sailry.platform"];
                match serde_json::from_value::<Extension>(value.clone()) {
                    Ok(parsed) if parsed.api_version != sailry_protocol::plugin::API_VERSION => {
                        issues.push(Issue {
                            path: "plugin.json.extensions.dev.sailry.platform".into(),
                            kind: IssueKind::UnsupportedExtension,
                        });
                    }
                    Ok(parsed)
                        if parsed.actions.len() <= 32
                            && parsed.actions.iter().all(sailry_protocol::plugin::Action::supported)
                            && parsed.files_in_namespace()
                            && parsed.instructions.as_ref().is_none_or(|text| {
                                !text.trim().is_empty() && text.len() <= 16 * 1024
                            })
                            && sailry_protocol::plugin::model_tools::valid(&parsed.model_tools)
                            && parsed.host.as_ref().is_none_or(|host| host.valid())
                            && parsed.display.as_ref().is_none_or(
                                sailry_protocol::plugin::desktop::Navigation::valid,
                            )
                            && parsed.description.as_ref().is_none_or(
                                sailry_protocol::plugin::desktop::Navigation::valid,
                            )
                            && sailry_protocol::plugin::valid_icon(&parsed.icon)
                            && sailry_protocol::plugin::ui::valid(&parsed.ui)
                            && parsed.storage.as_ref().is_none_or(|storage| super::storage::validate(storage).is_ok())
                            && parsed.ui.iter().all(|entry| entry.command.as_ref().is_none_or(|command| {
                                command.kind != sailry_protocol::plugin::ui::CommandKind::Message
                                    || parsed.host.as_ref().is_some_and(|host| host.commands.iter().any(|route| route.name == command.name))
                            }))
                            && (parsed.ui.is_empty()
                                || parsed
                                    .desktop
                                    .as_ref()
                                    .is_some_and(|desktop| desktop.ui_entry.is_some())
                                )
                            && parsed.tools.len() <= 256
                            && parsed.tools.iter().enumerate().all(|(index, tool)| {
                                tool.valid()
                                    && tool.handler.as_ref().is_none_or(|handler| {
                                        parsed.host.as_ref().is_some_and(|host| {
                                            host.handlers.contains(&handler.name)
                                                && handler
                                                    .result
                                                    .as_ref()
                                                    .is_none_or(|name| host.handlers.contains(name))
                                        }) && handler.flow.as_ref().is_none_or(|flow| {
                                            flow.operations.iter().all(|operation| {
                                                operation.action().is_none_or(|action| {
                                                    parsed.actions.contains(&action)
                                                })
                                            })
                                        }) && handler
                                            .operation
                                            .and_then(|operation| operation.action())
                                            .is_none_or(|action| parsed.actions.contains(&action))
                                    })
                                    && tool
                                        .operation
                                        .and_then(|operation| operation.action())
                                        .is_none_or(|action| parsed.actions.contains(&action))
                                    && !parsed.tools[..index].iter().any(|other| {
                                        other.name == tool.name && other.server == tool.server
                                    })
                            })
                            && parsed.actions.iter().enumerate().all(|(index, action)| {
                                !parsed.actions[..index].contains(action)
                            })
                            && parsed.desktop.as_ref().is_none_or(|desktop| {
                                desktop.valid()
                                    && (desktop.navigation_options.surface != sailry_protocol::plugin::desktop::Surface::Settings || parsed.settings_page.as_ref().is_some_and(|page|page.entry.is_some()))
                                    && (desktop.previews.is_empty()
                                        || parsed.scope == sailry_protocol::plugin::Scope::Host
                                            && parsed.actions.contains(&sailry_protocol::plugin::Action::ReadFiles))
                                    && (desktop.navigation_options.target
                                        != sailry_protocol::plugin::desktop::NavigationTarget::Worktree
                                        || parsed.scope == sailry_protocol::plugin::Scope::Host)
                                    && desktop
                                        .renderers
                                        .iter()
                                        .all(|renderer| renderer.valid(&parsed))
                                    && (desktop.entry.is_some()
                                        || desktop.ui_entry.is_some()
                                            && desktop.navigation.is_none()
                                            && desktop.panel.is_none()
                                            && desktop.conversations.is_empty()
                                            && desktop.renderers.is_empty()
                                        || parsed
                                            .settings_page
                                            .as_ref()
                                            .is_some_and(|page| page.entry.is_some())
                                            && desktop.navigation.is_none()
                                            && desktop.panel.is_none()
                                            && desktop.conversations.is_empty()
                                            && desktop.renderers.is_empty())
                            }) =>
                    {
                        if let Some(page) = &parsed.settings_page {
                            let valid = page.navigation.valid()
                                && page.entry.as_ref().is_none_or(|entry| {
                                    (entry.ends_with(".js") || entry.ends_with(".mjs"))
                                        && parsed.desktop.as_ref().is_some_and(|desktop| {
                                            desktop.resources.contains(entry)
                                        })
                                });
                            if !valid {
                                issues.push(Issue {
                                    path:
                                        "plugin.json.extensions.dev.sailry.platform.settings_page"
                                            .into(),
                                    kind: IssueKind::InvalidExtensions,
                                });
                            } else {
                                extension = Some(parsed);
                            }
                        } else {
                            extension = Some(parsed);
                        }
                    }
                    _ => issues.push(Issue {
                        path: "plugin.json.extensions.dev.sailry.platform".into(),
                        kind: IssueKind::InvalidExtensions,
                    }),
                }
            }
            // Unknown client namespaces have no validation or loading semantics here.
            _ => {}
        }
    }
    let version = string(object, "version")?.map(str::to_owned);
    let description = string(object, "description")?.map(str::to_owned);
    Ok(Manifest {
        name: name.into(),
        version,
        description,
        issues,
        extension,
    })
}

pub(super) fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, Fault> {
    object
        .get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| invalid("plugin metadata field must be a string"))
        })
        .transpose()
}

pub(crate) fn validate_name(name: &str) -> Result<(), Fault> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        })
        || !name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        || name.contains("--")
        || name.contains("..")
    {
        return Err(invalid("invalid Agent Plugins name"));
    }
    Ok(())
}

#[cfg(test)]
mod authoring;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_standard_display_metadata() {
        let value = json!({"$schema":SCHEMA, "name":"metadata", "version":"0.1.0".repeat(200),
            "description":"description ".repeat(600)});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() < 64 * 1024);
        let manifest = parse(&bytes).unwrap();
        assert_eq!(manifest.version.as_deref(), value["version"].as_str());
        assert_eq!(
            manifest.description.as_deref(),
            value["description"].as_str()
        );
        assert!(manifest.issues.is_empty());
    }

    #[test]
    fn confines_client_files_to_the_namespace() {
        let extension = json!({"api_version":"v1", "actions":[],
            "icon":"dev.sailry.platform/icon.png", "settings_schema":"dev.sailry.platform/settings.json",
            "host":{"entry":"dev.sailry.platform/host/main.js", "resources":["dev.sailry.platform/host/main.js"], "handlers":["run"]},
            "desktop":{"entry":"dev.sailry.platform/desktop/main.js", "resources":["dev.sailry.platform/desktop/main.js","dev.sailry.platform/desktop/settings.js"]},
            "settings_page":{"navigation":{"label":"Settings"},"entry":"dev.sailry.platform/desktop/settings.js"}
        });
        let check = |extension: Value| {
            parse(&serde_json::to_vec(&json!({
            "$schema":SCHEMA,"name":"namespaced", "extensions":{"dev.sailry.platform":extension}
        })).unwrap()).unwrap()
        };
        assert!(check(extension.clone()).extension.is_some());
        for pointer in [
            "/icon",
            "/settings_schema",
            "/host/entry",
            "/host/resources/0",
            "/desktop/entry",
            "/desktop/resources/0",
            "/settings_page/entry",
        ] {
            let mut invalid = extension.clone();
            let path = invalid.pointer_mut(pointer).unwrap();
            *path = path
                .as_str()
                .unwrap()
                .strip_prefix("dev.sailry.platform/")
                .unwrap()
                .into();
            let manifest = check(invalid);
            assert!(manifest.extension.is_none(), "accepted {pointer}");
            assert_eq!(manifest.issues[0].kind, IssueKind::InvalidExtensions);
        }
    }

    #[test]
    fn unsupported_capabilities_disable_the_extension() {
        for name in ["unknown.action", "models.decide"] {
            let value = json!({
                "$schema": SCHEMA,
                "name": "fixture",
                "extensions": {"dev.sailry.platform": {
                    "api_version": "v1",
                    "actions": ["files.read", name]
                }}
            });
            let manifest = parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(manifest.extension.is_none());
            assert_eq!(manifest.issues[0].kind, IssueKind::InvalidExtensions);
        }
    }

    #[test]
    fn omitted_capabilities_grant_nothing() {
        let value = json!({
            "$schema": SCHEMA,
            "name": "fixture",
            "extensions": {"dev.sailry.platform": {"api_version": "v1", "unused": true}}
        });
        let manifest = parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(manifest.issues.is_empty());
        assert!(manifest.extension.unwrap().actions.is_empty());
    }
    #[test]
    fn worktree_navigation_belongs_to_the_execution_node() {
        let mut value = json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"workspace", "extensions":{"dev.sailry.platform":{
                "api_version":"v1", "scope":"host", "actions":[],
                "desktop":{
                    "entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"],
                    "navigation":{"label":"Workspace","icon":"reicon:folders/folder"},
                    "navigation_options":{"target":"worktree","details":true}
                }
            }}
        });
        assert!(
            parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .issues
                .is_empty()
        );
        value["extensions"]["dev.sailry.platform"]["scope"] = json!("desktop");
        assert!(
            !parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .issues
                .is_empty()
        );
    }
    #[test]
    fn accepts_settings_only_resources() {
        let bytes = include_bytes!("../../../../plugins/computer/plugin.json");
        let parsed = parse(bytes).unwrap();
        assert!(parsed.issues.is_empty(), "{:?}", parsed.issues);
        let extension = parsed.extension.unwrap();
        assert!(extension.desktop.as_ref().unwrap().entry.is_none());
        assert!(extension.settings_page.as_ref().unwrap().entry.is_some());
        let original: Value = serde_json::from_slice(bytes).unwrap();
        for field in ["navigation", "panel"] {
            let mut value = original.clone();
            value["extensions"]["dev.sailry.platform"]["desktop"][field] =
                json!({"label":"Unavailable workspace","icon":"monitor"});
            assert!(
                !parse(&serde_json::to_vec(&value).unwrap())
                    .unwrap()
                    .issues
                    .is_empty()
            );
        }
        let mut missing = original;
        missing["extensions"]["dev.sailry.platform"]["settings_page"]["entry"] =
            json!("missing.js");
        assert!(
            !parse(&serde_json::to_vec(&missing).unwrap())
                .unwrap()
                .issues
                .is_empty()
        );
    }
}
