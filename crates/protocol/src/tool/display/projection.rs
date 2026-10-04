//! Finite data projections captured with the call; output bytes never enter a VM.
use crate::plugin::desktop::Navigation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
mod table;
pub use table::{Bytes, Cell, Table};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub preview: Option<Text>,
    #[serde(default)]
    pub notices: Vec<Notice>,
    pub body: Option<Body>,
    pub paths: Option<Paths>,
    pub table: Option<Table>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub source: Source,
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Arguments,
    Result,
    Item,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub parts: Vec<Part>,
    #[serde(default)]
    pub separator: String,
}

/// Exactly one of text, value or rows supplies a part. Rows cannot nest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub take: Option<usize>,
    pub text: Option<String>,
    pub value: Option<Reference>,
    pub rows: Option<Rows>,
    pub when: Option<Condition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rows {
    pub value: Reference,
    pub text: Text,
    pub separator: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(default)]
    pub any: Vec<Comparison>,
    #[serde(default)]
    pub all: Vec<Comparison>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub value: Reference,
    /// Compare any array item at this relative pointer.
    pub item: Option<String>,
    /// Missing references compare as null, matching optional JSON fields.
    pub equals: Value,
    #[serde(default)]
    pub negate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub message: Navigation,
    pub when: Option<Condition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub text: Reference,
    pub path: Option<Reference>,
    pub diff: Option<Diff>,
    pub empty: Option<Navigation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Diff {
    pub format: Format,
    pub when: Option<Condition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Added,
    Unified,
}

/// A string or an array of items containing paths; the call supplies ownership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub value: Reference,
    pub item: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub message: Navigation,
    #[serde(default)]
    pub values: BTreeMap<String, Reference>,
    pub when: Option<Condition>,
}

#[derive(Clone, Copy)]
struct Data<'a> {
    arguments: &'a Value,
    result: &'a Value,
    item: Option<&'a Value>,
}

impl<'a> Data<'a> {
    fn new(arguments: &'a Value, result: &'a Value) -> Self {
        Self {
            arguments,
            result,
            item: None,
        }
    }
}

impl Reference {
    fn valid(&self, item: bool) -> bool {
        (self.source != Source::Item || item)
            && (self.path.is_empty() || super::valid_pointer(&self.path))
    }

    fn resolve<'a>(&self, data: Data<'a>) -> Option<&'a Value> {
        match self.source {
            Source::Arguments => data.arguments,
            Source::Result => data.result,
            Source::Item => data.item?,
        }
        .pointer(&self.path)
    }

    pub fn read<'a>(&self, arguments: &'a Value, result: &'a Value) -> Option<&'a Value> {
        self.resolve(Data::new(arguments, result))
    }
}

impl Condition {
    fn valid(&self, item: bool) -> bool {
        (!self.any.is_empty() || !self.all.is_empty())
            && self.any.len() + self.all.len() <= 8
            && self.any.iter().chain(&self.all).all(|test| {
                test.value.valid(item)
                    && test
                        .item
                        .as_ref()
                        .is_none_or(|path| path.is_empty() || super::valid_pointer(path))
                    && !test.equals.is_array()
                    && !test.equals.is_object()
                    && test.equals.as_str().is_none_or(|value| value.len() <= 1024)
            })
    }

    fn resolve(&self, data: Data<'_>) -> bool {
        let matches = |test: &Comparison| {
            let value = test.value.resolve(data).unwrap_or(&Value::Null);
            let matched = match &test.item {
                Some(path) => value.as_array().is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.pointer(path).unwrap_or(&Value::Null) == &test.equals)
                }),
                None => value == &test.equals,
            };
            matched != test.negate
        };
        (self.any.is_empty() || self.any.iter().any(matches)) && self.all.iter().all(matches)
    }

    pub fn matches(&self, arguments: &Value, result: &Value) -> bool {
        self.resolve(Data::new(arguments, result))
    }
}

impl Text {
    fn valid(&self, item: bool) -> bool {
        !self.parts.is_empty()
            && self.parts.len() <= 32
            && self.separator.len() <= 128
            && self.parts.iter().all(|part| {
                usize::from(part.text.is_some())
                    + usize::from(part.value.is_some())
                    + usize::from(part.rows.is_some())
                    == 1
                    && part
                        .take
                        .is_none_or(|limit| part.value.is_some() && limit > 0 && limit <= 4096)
                    && part.text.as_ref().is_none_or(|text| text.len() <= 1024)
                    && part.value.as_ref().is_none_or(|value| value.valid(item))
                    && part.when.as_ref().is_none_or(|test| test.valid(item))
                    && part.rows.as_ref().is_none_or(|rows| {
                        !item
                            && rows.value.valid(false)
                            && rows.text.valid(true)
                            && rows.separator.len() <= 128
                    })
            })
    }

    fn resolve(&self, data: Data<'_>) -> String {
        self.parts
            .iter()
            .filter(|part| part.when.as_ref().is_none_or(|test| test.resolve(data)))
            .filter_map(|part| {
                if let Some(text) = &part.text {
                    return Some(text.clone());
                }
                if let Some(value) = &part.value {
                    return value.resolve(data).and_then(literal).map(|text| {
                        part.take.map_or_else(
                            || text.clone(),
                            |limit| text.chars().take(limit).collect(),
                        )
                    });
                }
                let rows = part.rows.as_ref()?;
                let values = rows.value.resolve(data)?.as_array()?;
                if values.is_empty() {
                    return None;
                }
                Some(
                    values
                        .iter()
                        .map(|item| {
                            rows.text.resolve(Data {
                                item: Some(item),
                                ..data
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(&rows.separator),
                )
            })
            .collect::<Vec<_>>()
            .join(&self.separator)
    }

    pub fn render(&self, arguments: &Value, result: &Value) -> String {
        self.resolve(Data::new(arguments, result))
    }
}

fn literal(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(_) | Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

impl Paths {
    fn valid(&self) -> bool {
        self.value.valid(false)
            && self
                .item
                .as_ref()
                .is_none_or(|path| path.is_empty() || super::valid_pointer(path))
    }

    pub fn read<'a>(&self, arguments: &'a Value, result: &'a Value) -> Vec<&'a str> {
        let Some(value) = self.value.read(arguments, result) else {
            return Vec::new();
        };
        if let Some(item) = &self.item {
            return value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.pointer(item)?.as_str())
                .filter(|path| !path.is_empty())
                .collect();
        }
        value
            .as_str()
            .filter(|path| !path.is_empty())
            .into_iter()
            .collect()
    }
}

impl Output {
    pub fn valid(&self) -> bool {
        (self.preview.is_some()
            || !self.notices.is_empty()
            || self.body.is_some()
            || self.paths.is_some()
            || self.table.is_some())
            && self.preview.as_ref().is_none_or(|text| text.valid(false))
            && self.notices.len() <= 8
            && self.notices.iter().all(|notice| {
                notice.message.valid() && notice.when.as_ref().is_none_or(|test| test.valid(false))
            })
            && self.paths.as_ref().is_none_or(Paths::valid)
            && self.table.as_ref().is_none_or(Table::valid)
            && self.body.as_ref().is_none_or(|body| {
                body.text.valid(false)
                    && body.path.as_ref().is_none_or(|value| value.valid(false))
                    && body.empty.as_ref().is_none_or(Navigation::valid)
                    && body
                        .diff
                        .as_ref()
                        .is_none_or(|diff| diff.when.as_ref().is_none_or(|test| test.valid(false)))
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= 32 * 1024)
    }

    pub fn notices(&self, arguments: &Value, result: &Value, locale: &str) -> Vec<String> {
        let mut labels = Vec::new();
        for notice in &self.notices {
            if notice
                .when
                .as_ref()
                .is_none_or(|test| test.matches(arguments, result))
            {
                let label = notice.message.label(locale).to_owned();
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        labels
    }
}

impl Prompt {
    pub fn valid(&self) -> bool {
        self.message.valid()
            && self.values.len() <= 8
            && self.values.iter().all(|(name, value)| {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    && value.source == Source::Arguments
                    && value.valid(false)
            })
            && self.when.as_ref().is_none_or(|test| {
                test.valid(false)
                    && test
                        .any
                        .iter()
                        .chain(&test.all)
                        .all(|test| test.value.source == Source::Arguments)
            })
    }

    pub fn render(&self, arguments: &Value, locale: &str) -> Option<String> {
        if self
            .when
            .as_ref()
            .is_some_and(|test| !test.matches(arguments, &Value::Null))
        {
            return None;
        }
        let mut template = self.message.label(locale);
        let mut text = String::new();
        while let Some(start) = template.find("%{") {
            text.push_str(&template[..start]);
            let token = &template[start + 2..];
            let Some(end) = token.find('}') else {
                text.push_str(&template[start..]);
                return Some(text);
            };
            match self
                .values
                .get(&token[..end])
                .and_then(|value| value.read(arguments, &Value::Null))
                .and_then(literal)
            {
                Some(value) => text.push_str(&value),
                None => text.push_str(&template[start..start + end + 3]),
            }
            template = &token[end + 1..];
        }
        text.push_str(template);
        Some(text)
    }
}

#[cfg(test)]
mod tests;
