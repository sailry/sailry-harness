//! Conversation rows carry bounded facts; canonical tool values are read explicitly.
use super::*;
use sailry_protocol::tool::{
    Display, Failure, Grouping, Presentation, ResultDisplay,
    detail::{Delegation, Detail, Resolved, Summary},
};
mod cache;
mod display;
mod read;
#[cfg(test)]
mod tests;
pub(in crate::store) use cache::Cache;
pub(in crate::store) use read::page;
pub(in crate::store::agent) use read::{Sources, raw, with_sources};

#[derive(Clone)]
pub(in crate::store::agent) struct Raw {
    source: Option<u64>,
    pub id: Option<String>,
    pub name: String,
    pub arguments: Value,
    pub result: Option<Value>,
    pub display: Option<Display>,
    pub presentation: Presentation,
    pub grouping: Grouping,
    pub images: Vec<sailry_protocol::conversation::Image>,
    pub failure: Option<Failure>,
}

impl Raw {
    pub(in crate::store::agent) fn new(id: Option<String>, name: String, arguments: Value) -> Self {
        Self {
            source: None,
            id,
            name,
            arguments,
            result: None,
            display: None,
            presentation: Default::default(),
            grouping: Default::default(),
            images: Vec::new(),
            failure: None,
        }
    }

    pub(super) fn summary(&self) -> Summary {
        let result = self.result.as_ref().unwrap_or(&Value::Null);
        let mut resolved = self.resolved(false);
        let metadata = ResultDisplay::from_value(result);
        let diagnostics = self.failure.is_some()
            || metadata
                .as_ref()
                .is_some_and(|metadata| !metadata.diagnostics.is_empty());
        let content = content(self);
        let visible = if let Some(content) = &content {
            content.has_details(result)
        } else if resolved
            .as_ref()
            .is_some_and(|resolved| resolved.output.is_some())
        {
            output_visible(self, resolved.as_ref())
        } else {
            self.display
                .as_ref()
                .is_none_or(|display| display.output.is_none())
                && literal(result).is_some()
        };
        let has_details = self.result.is_some()
            && (diagnostics
                || !self.images.is_empty()
                || progress(self).is_some()
                || !matches!(
                    self.presentation,
                    Presentation::Summary | Presentation::Hidden
                ) && (self
                    .display
                    .as_ref()
                    .is_some_and(|display| display.raw_result)
                    || visible));
        if let Some(resolved) = &mut resolved {
            bound(resolved);
        }
        let question = matches!(
            self.name.as_str(),
            "ask_user" | "mcp_elicitation" | "plan_review"
        )
        .then(|| serde_json::from_value::<question::Spec>(self.arguments.clone()).ok())
        .flatten()
        .filter(|spec| spec.validate().is_ok())
        .map(Box::new);
        Summary {
            revision: self.revision(),
            icon: self
                .display
                .as_ref()
                .and_then(|display| display.icon.clone()),
            resolved,
            has_details,
            published: content.as_ref().is_some_and(|content| {
                content.blocks.iter().any(|block| {
                    matches!(
                        block,
                        sailry_protocol::tool::Block::File(_)
                            | sailry_protocol::tool::Block::Artifact { .. }
                            | sailry_protocol::tool::Block::Card(_)
                    )
                })
            }),
            failure: self.failure.clone(),
            diagnostics,
            expected: metadata.as_ref().and_then(|metadata| metadata.expected),
            error: error(self, metadata.as_ref()).map(|error| clip(&error, 1024, false)),
            raw_result: self
                .display
                .as_ref()
                .is_some_and(|display| display.raw_result),
            images: self.images.len(),
            bytes: self.result.as_ref().map(|result| result.to_string().len()),
            lines: result
                .get("text")
                .and_then(Value::as_str)
                .map(|text| text.lines().count()),
            path: self
                .arguments
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty() && !path.contains(['\r', '\n']))
                .map(|path| clip(path, 200, true)),
            title: text(&self.arguments, "title", 200),
            resource: text(&self.arguments, "skill", 200),
            summary_id: text(result, "summary_id", 200),
            delegation: delegation(&self.arguments),
            question,
            progress: progress(self),
        }
    }

    pub(in crate::store::agent) fn part(self) -> Part {
        let summary = Box::new(self.summary());
        if self.result.is_some() {
            Part::ToolResult {
                id: self.id,
                name: self.name,
                summary,
                images: self.images,
            }
        } else {
            Part::ToolCall {
                id: self.id,
                name: self.name,
                summary,
                presentation: self.presentation,
                grouping: self.grouping,
            }
        }
    }

    fn revision(&self) -> String {
        let bytes = serde_json::to_vec(&(
            &self.id,
            &self.name,
            &self.arguments,
            &self.result,
            &self.display,
            self.presentation,
            self.grouping,
            &self.images,
            &self.failure,
        ))
        .expect("public tool values are serializable");
        blake3::hash(&bytes).to_hex().to_string()
    }

    pub(super) fn detail(&self) -> Detail {
        Detail {
            arguments: self.arguments.clone(),
            result: self.result.clone(),
            resolved: self.resolved(true),
            content: content(self),
            images: self.images.clone(),
        }
    }

    fn resolved(&self, full: bool) -> Option<Resolved> {
        let mut resolved = display::resolve(
            self.display.as_ref(),
            &self.arguments,
            self.result.as_ref().unwrap_or(&Value::Null),
            full,
        )?;
        if self.presentation != Presentation::Content || content(self).is_none() {
            resolved.command = None;
        }
        Some(resolved)
    }
}

fn delegation(arguments: &Value) -> Delegation {
    let session = arguments
        .get("session")
        .map(|value| serde_json::from_value(value.clone()));
    let worktree = arguments
        .get("worktree")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()));
    Delegation {
        invalid: session.as_ref().is_some_and(Result::is_err)
            || worktree.as_ref().is_some_and(Result::is_err),
        session: session.and_then(Result::ok),
        worktree: worktree.and_then(Result::ok),
    }
}

fn progress(raw: &Raw) -> Option<sailry_protocol::conversation::progress::Progress> {
    use sailry_protocol::conversation::progress::Progress;
    if raw.presentation != Presentation::Progress {
        return None;
    }
    let arguments: Progress = serde_json::from_value(raw.arguments.clone()).ok()?;
    arguments.validate().ok()?;
    let result = raw.result.as_ref()?.as_object()?;
    if result.len() != 1 {
        return None;
    }
    let progress: Progress = serde_json::from_value(result.get("progress")?.clone()).ok()?;
    (progress == arguments).then_some(progress)
}

fn content(raw: &Raw) -> Option<sailry_protocol::tool::Content> {
    let content = raw.presentation.content(raw.result.as_ref()?)?;
    if raw.failure.is_some()
        && !content.blocks.iter().all(|block| {
            matches!(
                block,
                sailry_protocol::tool::Block::Text(_)
                    | sailry_protocol::tool::Block::Notice { .. }
                    | sailry_protocol::tool::Block::Card(_)
            )
        })
    {
        return None;
    }
    content
        .blocks
        .iter()
        .all(|block| match block {
            sailry_protocol::tool::Block::Image { index } => {
                raw.images.iter().any(|image| image.index == *index)
            }
            _ => true,
        })
        .then_some(content)
}

fn output_visible(raw: &Raw, resolved: Option<&Resolved>) -> bool {
    let Some(recipe) = raw
        .display
        .as_ref()
        .and_then(|display| display.output.as_ref())
    else {
        return false;
    };
    let result = raw.result.as_ref().unwrap_or(&Value::Null);
    resolved
        .and_then(|resolved| resolved.output.as_ref())
        .is_some_and(|output| {
            output.preview_deferred
                || !output.preview.trim().is_empty()
                || !output.notices.is_empty()
                || !output.paths.is_empty()
        })
        || recipe.table.is_some()
        || recipe.body.as_ref().is_some_and(|body| {
            body.text
                .read(&raw.arguments, result)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty() || body.empty.is_some())
        })
}

fn error(raw: &Raw, metadata: Option<&ResultDisplay>) -> Option<String> {
    if let Some(metadata) = metadata.filter(|metadata| !metadata.diagnostics.is_empty()) {
        return Some(
            metadata
                .diagnostics
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    raw.failure.as_ref()?;
    let result = raw.result.as_ref()?;
    result
        .pointer("/error/message")
        .or_else(|| result.get("error"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn text(value: &Value, field: &str, limit: usize) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(|text| clip(text, limit, true))
}

pub(super) fn literal(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Array(items) if items.iter().all(Value::is_string) => {
            let text = items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}

fn clip(text: &str, limit: usize, single_line: bool) -> String {
    let mut result = String::new();
    for (index, character) in text.chars().enumerate() {
        if index == limit {
            break;
        }
        if single_line && matches!(character, '\n' | '\r') {
            break;
        }
        result.push(character);
    }
    result
}

fn local(value: &mut sailry_protocol::tool::LocalText, limit: usize) {
    value.label = clip(&value.label, limit, true);
    value.locales = std::mem::take(&mut value.locales)
        .into_iter()
        .map(|(locale, text)| (clip(&locale, 32, true), clip(&text, limit, true)))
        .collect();
}

fn bound(value: &mut Resolved) {
    if let Some(label) = &mut value.label {
        local(label, 200);
    }
    for field in [
        &mut value.input.summary,
        &mut value.input.context,
        &mut value.input.content,
        &mut value.input.target,
    ]
    .into_iter()
    .flatten()
    {
        field.text = clip(&field.text, 200, true);
    }
    value.approval.truncate(4);
    for prompt in &mut value.approval {
        local(prompt, 1024);
    }
    if let Some(status) = &mut value.status {
        local(status, 200);
    }
    value.diagnostics.truncate(4);
    for diagnostic in &mut value.diagnostics {
        diagnostic.text = clip(&diagnostic.text, 256, false);
    }
    if let Some(output) = &mut value.output {
        output.preview = clip(&output.preview, 2048, false);
        output.paths.retain(|path| !path.contains(['\r', '\n']));
        output.paths.truncate(32);
        for path in &mut output.paths {
            *path = clip(path, 200, true);
        }
        output.notices.truncate(4);
        for notice in &mut output.notices {
            local(notice, 200);
        }
        output.body = None;
        output.table = None;
    }
}
