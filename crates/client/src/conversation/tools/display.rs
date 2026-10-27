//! Controllers consume Node-resolved presentation; recipes remain on the Node.
use super::Call;
use sailry_protocol::conversation::{Child, Page, Part};
pub use sailry_protocol::tool::detail::{Body, Input, Output, Resolved, Text};
use serde_json::Value;
use std::borrow::Cow;

impl Call {
    pub fn output(&self, page: &Page) -> Option<Cow<'_, Output>> {
        if self.diagnostics(page) {
            return None;
        }
        if let Some(output) = self
            .resolved()
            .and_then(|resolved| resolved.output.as_ref())
        {
            return Some(Cow::Borrowed(output));
        }
        Some(Cow::Owned(Output {
            preview: literal(self.result(page)?)?.into_owned(),
            preview_deferred: false,
            notices: Vec::new(),
            paths: Vec::new(),
            path_count: 0,
            body: None,
            table: None,
        }))
    }
}

/// Literal rendering of explicitly loaded public details, without schema guessing.
pub fn literal(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::String(text) if !text.is_empty() => Some(Cow::Borrowed(text)),
        Value::Array(items)
            if items.iter().all(Value::is_string)
                && items
                    .iter()
                    .any(|item| item.as_str().is_some_and(|text| !text.is_empty())) =>
        {
            Some(Cow::Owned(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        Value::Number(_) | Value::Bool(_) => Some(Cow::Owned(value.to_string())),
        _ => None,
    }
}

pub fn child_title<'a>(page: Option<&'a Page>, child: &'a Child) -> Option<Cow<'a, str>> {
    let name = child.name.as_deref().filter(|name| !name.trim().is_empty());
    let title = page
        .and_then(|page| {
            page.entries
                .iter()
                .find(|entry| entry.id == child.origin.entry && entry.turn == child.origin.turn)
        })
        .and_then(|entry| entry.parts.get(child.origin.index))
        .and_then(|part| match part {
            Part::ToolCall { summary, .. } => summary.title.as_deref(),
            _ => None,
        })
        .filter(|title| !title.trim().is_empty());
    match (title, name) {
        (Some(title), Some(name)) if child.origin.role.is_some() => {
            Some(Cow::Owned(format!("{name} {title}")))
        }
        (Some(title), _) => Some(Cow::Borrowed(title)),
        (None, Some(name)) => Some(Cow::Borrowed(name)),
        _ => None,
    }
}
