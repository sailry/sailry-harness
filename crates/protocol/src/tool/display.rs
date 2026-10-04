//! Captured argument fields are data references, never scripts or mutable package lookups.
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
}

impl Field {
    pub fn valid(&self) -> bool {
        valid_pointer(&self.path)
            && self.omit.len() <= 16
            && self.omit.iter().all(|value| value.len() <= 256)
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
    fn rejects_invalid_selectors() {
        for path in ["", "command", "/~", "/~2", "/bad\npath"] {
            let field = Field {
                path: path.into(),
                code: false,
                omit: vec![],
            };
            assert!(!field.valid(), "{path:?}");
        }
        let field = Field {
            path: "/command".into(),
            code: false,
            omit: vec![],
        };
        assert!(
            field
                .read(&json!({"command":{"text":"not a string"}}))
                .is_none()
        );
        assert!(field.read(&json!({"command":"  "})).is_none());
    }
}
