//! Derived from Goose 14741e9a, goose-context-management/src/structured.rs.
//! Apache-2.0; see third_party_licenses/goose-Apache-2.0.txt.
use super::{RENDER, templates};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod json;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Summary {
    #[serde(default, deserialize_with = "strings")]
    pub user_intent: Vec<String>,
    #[serde(default, deserialize_with = "strings")]
    pub technical_concepts: Vec<String>,
    #[serde(default, deserialize_with = "files")]
    pub files: Vec<File>,
    #[serde(default, deserialize_with = "strings")]
    pub errors_and_fixes: Vec<String>,
    #[serde(default, deserialize_with = "strings")]
    pub problem_solving: Vec<String>,
    #[serde(default, deserialize_with = "strings")]
    pub user_messages: Vec<String>,
    #[serde(default, deserialize_with = "strings")]
    pub pending_tasks: Vec<String>,
    #[serde(default, deserialize_with = "optional")]
    pub current_work: Option<String>,
    #[serde(default, deserialize_with = "optional")]
    pub next_step: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct File {
    #[serde(default, deserialize_with = "text")]
    pub path: String,
    #[serde(default, deserialize_with = "text")]
    pub summary: String,
    #[serde(default, deserialize_with = "optional")]
    pub key_code: Option<String>,
}

impl Summary {
    pub(super) fn parse(text: &str) -> Option<Self> {
        json::candidates(text).into_iter().find_map(|candidate| {
            let value = json::value(candidate).ok()?;
            let mut summary: Self = serde_json::from_value(value).ok()?;
            summary.normalize();
            (!summary.is_empty()).then_some(summary)
        })
    }

    pub(super) fn render(&self) -> Result<String, minijinja::Error> {
        templates::render(RENDER, self)
    }

    fn normalize(&mut self) {
        for list in [
            &mut self.user_intent,
            &mut self.technical_concepts,
            &mut self.errors_and_fixes,
            &mut self.problem_solving,
            &mut self.user_messages,
            &mut self.pending_tasks,
        ] {
            list.retain(|text| !text.trim().is_empty());
        }
        for file in &mut self.files {
            if file.key_code.as_deref().is_some_and(blank) {
                file.key_code = None;
            }
        }
        self.files
            .retain(|file| !blank(&file.path) || !blank(&file.summary) || file.key_code.is_some());
        if self.current_work.as_deref().is_some_and(blank) {
            self.current_work = None;
        }
        if self.next_step.as_deref().is_some_and(blank) {
            self.next_step = None;
        }
    }

    fn is_empty(&self) -> bool {
        self.user_intent.is_empty()
            && self.technical_concepts.is_empty()
            && self.files.is_empty()
            && self.errors_and_fixes.is_empty()
            && self.problem_solving.is_empty()
            && self.user_messages.is_empty()
            && self.pending_tasks.is_empty()
            && self.current_work.is_none()
            && self.next_step.is_none()
    }
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}: {}", stringify(value)))
            .collect::<Vec<_>>()
            .join("; "),
        Value::Array(items) => items.iter().map(stringify).collect::<Vec<_>>().join("; "),
        other => other.to_string(),
    }
}

fn strings<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<Vec<String>, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(items) => items.iter().map(stringify).collect(),
        Value::Null => Vec::new(),
        other => vec![stringify(&other)],
    })
}

fn text<'de, Deserializer>(deserializer: Deserializer) -> Result<String, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(stringify(&value))
}

fn optional<'de, Deserializer>(
    deserializer: Deserializer,
) -> Result<Option<String>, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        Value::Null => None,
        other => Some(stringify(&other)),
    })
}

fn files<'de, Deserializer>(deserializer: Deserializer) -> Result<Vec<File>, Deserializer::Error>
where
    Deserializer: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let items = match value {
        Value::Array(items) => items,
        Value::Null => return Ok(Vec::new()),
        other => vec![other],
    };
    Ok(items
        .into_iter()
        .filter_map(|item| match item {
            Value::Object(_) => serde_json::from_value(item).ok(),
            other => {
                let path = stringify(&other);
                (!path.trim().is_empty()).then_some(File {
                    path,
                    summary: String::new(),
                    key_code: None,
                })
            }
        })
        .collect())
}
