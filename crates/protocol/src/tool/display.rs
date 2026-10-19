//! Captured fields are data references, never scripts or mutable package lookups.
use crate::plugin::desktop::{Icon, Navigation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub mod projection;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Display {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(default)]
    pub locales: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<projection::Output>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approval: Vec<projection::Prompt>,
    /// Execution Node paths captured for display only, never package-authored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(skip)]
    pub aliases: Vec<Alias>,
}

impl Display {
    pub fn label(&self, locale: &str) -> &str {
        self.locales
            .get(locale)
            .map(String::as_str)
            .unwrap_or(&self.label)
    }

    pub fn valid(&self) -> bool {
        Navigation {
            label: self.label.clone(),
            icon: self.icon.clone(),
            locales: self.locales.clone(),
        }
        .valid()
            && self.input.as_ref().is_none_or(Input::valid)
            && self.output.as_ref().is_none_or(projection::Output::valid)
            && self.approval.len() <= 4
            && self.approval.iter().all(projection::Prompt::valid)
            && self.aliases.len() <= 32
            && self.aliases.iter().all(Alias::valid)
    }
}

impl From<Navigation> for Display {
    fn from(value: Navigation) -> Self {
        Self {
            label: value.label,
            icon: value.icon,
            locales: value.locales,
            input: None,
            output: None,
            approval: Vec::new(),
            aliases: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub summary: Option<Field>,
    pub context: Option<Field>,
    pub target: Option<Field>,
    pub content: Option<Field>,
}

impl Input {
    pub fn valid(&self) -> bool {
        [&self.summary, &self.context, &self.target, &self.content]
            .into_iter()
            .all(|field| field.as_ref().is_none_or(Field::valid))
            && [&self.context, &self.target, &self.content]
                .into_iter()
                .all(|field| {
                    field.as_ref().is_none_or(|field| {
                        field.format.is_none()
                            && field.result_path.is_none()
                            && field.names.is_empty()
                    })
                })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// JSON Pointer into the original, authoritative arguments.
    pub path: String,
    #[serde(default)]
    pub code: bool,
    #[serde(default)]
    pub omit: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<InputFormat>,
    /// Optional result name for the summary, falling back to the original argument.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_path: Option<String>,
    /// Public resource names captured by the execution Node, never package-authored.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(skip)]
    pub names: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputFormat {
    Command,
    Session,
    Connection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub path: String,
    pub kind: AliasKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AliasKind {
    Home,
    Python,
}

impl Alias {
    fn valid(&self) -> bool {
        let bytes = self.path.as_bytes();
        (self.path.starts_with('/')
            || self.path.starts_with("\\\\")
            || bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'/' | b'\\'))
            && self.path.len() <= 4096
            && !self.path.chars().any(char::is_control)
    }
}

impl Field {
    pub fn valid(&self) -> bool {
        valid_pointer(&self.path)
            && self.omit.len() <= 16
            && self.omit.iter().all(|value| value.len() <= 256)
            && self.result_path.as_deref().is_none_or(valid_pointer)
            && (self.names.is_empty()
                || matches!(
                    self.format,
                    Some(InputFormat::Connection | InputFormat::Session)
                ))
            && self.names.iter().all(|(id, name)| {
                !id.is_empty()
                    && !id.chars().any(char::is_control)
                    && (name.trim().is_empty() || !name.chars().any(char::is_control))
            })
    }

    pub fn read<'a>(&self, arguments: &'a Value) -> Option<&'a str> {
        let value = arguments.pointer(&self.path)?.as_str()?;
        (!value.trim().is_empty() && !self.omit.iter().any(|omitted| omitted == value))
            .then_some(value)
    }
}

pub(super) fn valid_pointer(path: &str) -> bool {
    if !path.starts_with('/') || path.len() > 1024 || path.chars().any(char::is_control) {
        return false;
    }
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reserves_aliases_for_captured_display() {
        let mut declaration: super::super::Declaration = serde_json::from_value(json!({
            "name":"run", "display":{"label":"Run","input":{"summary":{"path":"/command","format":"command"}}}
        })).unwrap();
        assert!(declaration.valid());
        declaration.display.as_mut().unwrap().aliases.push(Alias {
            path: "/node/home".into(),
            kind: AliasKind::Home,
        });
        assert!(declaration.display.as_ref().unwrap().valid());
        assert!(!declaration.valid());
        let input: Input =
            serde_json::from_value(json!({"content":{"path":"/command","format":"command"}}))
                .unwrap();
        assert!(!input.valid());
    }

    #[test]
    fn reads_original_fields() {
        let display: Display = serde_json::from_value(json!({
            "label":"Run", "input":{
                "summary":{"path":"/command", "code":true},
                "context":{"path":"/cwd", "omit":["."]},
                "content":{"path":"/escaped~1key/~0value"}
            }
        }))
        .unwrap();
        assert!(display.valid());
        let input = display.input.unwrap();
        let args =
            json!({"command":"first\nsecond", "cwd":".", "escaped/key":{"~value":"完整内容 🙂"}});
        assert_eq!(input.summary.unwrap().read(&args), Some("first\nsecond"));
        assert_eq!(input.context.unwrap().read(&args), None);
        assert_eq!(input.content.unwrap().read(&args), Some("完整内容 🙂"));
    }

    #[test]
    fn reserves_session_format_for_summaries() {
        let input: Input = serde_json::from_value(json!({
            "summary": {"path": "/session", "format": "session"}
        }))
        .unwrap();
        assert!(input.valid());
        assert_eq!(input.summary.unwrap().format, Some(InputFormat::Session));
        for field in ["content", "context", "target"] {
            let input: Input = serde_json::from_value(json!({
                field: {"path": "/session", "format": "session"}
            }))
            .unwrap();
            assert!(!input.valid());
        }
    }

    #[test]
    fn reserves_result_paths_for_summaries() {
        let input: Input = serde_json::from_value(json!({
            "summary": {"path": "/source/value", "result_path": "/data/path"}
        }))
        .unwrap();
        assert!(input.valid());
        for field in ["content", "context", "target"] {
            let input: Input = serde_json::from_value(json!({
                field: {"path": "/source/value", "result_path": "/data/path"}
            }))
            .unwrap();
            assert!(!input.valid());
        }
        for path in ["", "data/path", "/~", "/~2", "/bad\npath"] {
            let input: Input = serde_json::from_value(json!({
                "summary": {"path": "/source/value", "result_path": path}
            }))
            .unwrap();
            assert!(!input.valid());
        }
    }

    #[test]
    fn reserves_names_for_captured_summaries() {
        let mut declaration: super::super::Declaration = serde_json::from_value(json!({
            "name": "run", "display": {"label": "Run", "input": {
                "summary": {"path": "/connection", "format": "connection"}
            }}
        }))
        .unwrap();
        assert!(declaration.valid());
        let captured = serde_json::to_value(&declaration).unwrap();
        assert!(
            captured["display"]["input"]["summary"]
                .get("names")
                .is_none()
        );
        declaration
            .display
            .as_mut()
            .unwrap()
            .input
            .as_mut()
            .unwrap()
            .summary
            .as_mut()
            .unwrap()
            .names
            .insert("connection-id".into(), "Build host".into());
        assert!(declaration.display.as_ref().unwrap().valid());
        assert!(!declaration.valid());
        for field in ["context", "target", "content"] {
            let input: Input = serde_json::from_value(json!({field: {
                "path": "/connection", "names": {"connection-id": "Build host"}
            }}))
            .unwrap();
            assert!(!input.valid());
        }
    }

    #[test]
    fn rejects_invalid_selectors() {
        for path in ["", "command", "/~", "/~2", "/bad\npath"] {
            let field = Field {
                path: path.into(),
                code: false,
                omit: vec![],
                format: None,
                result_path: None,
                names: BTreeMap::new(),
            };
            assert!(!field.valid(), "{path:?}");
        }
        let field = Field {
            path: "/command".into(),
            code: false,
            omit: vec![],
            format: None,
            result_path: None,
            names: BTreeMap::new(),
        };
        assert!(
            field
                .read(&json!({"command":{"text":"not a string"}}))
                .is_none()
        );
        assert!(field.read(&json!({"command":"  "})).is_none());
    }
}
