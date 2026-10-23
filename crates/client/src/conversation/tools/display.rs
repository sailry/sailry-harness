//! Resolve captured data recipes once for UI-independent controller presentation.
use super::Call;
use sailry_protocol::{
    SessionId,
    conversation::{Child, Page, Part},
    plugin::desktop::Navigation,
    tool::{Diagnostic, Field, LocalText, ResultDisplay, Table, projection::Format},
};
use serde::Serialize;
use serde_json::Value;
use std::borrow::Cow;
mod command;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resolved {
    pub label: Option<LocalText>,
    pub input: Input,
    pub output: Option<Output>,
    pub approval: Vec<LocalText>,
    pub status: Option<LocalText>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Input {
    pub summary: Option<Text>,
    pub context: Option<Text>,
    pub target: Option<Text>,
    pub content: Option<Text>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Text {
    pub text: String,
    pub code: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Output {
    pub preview: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preview_deferred: bool,
    pub notices: Vec<LocalText>,
    pub paths: Vec<String>,
    pub body: Option<Body>,
    pub table: Option<Table>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Body {
    pub text: String,
    pub path: Option<String>,
    pub diff: Option<Format>,
    pub empty: Option<LocalText>,
}

fn label(value: &Navigation) -> LocalText {
    LocalText {
        label: value.label.clone(),
        locales: value.locales.clone(),
    }
}

fn field(value: Option<&Field>, arguments: &Value) -> Option<Text> {
    let value = value?;
    Some(Text {
        text: value.read(arguments)?.to_owned(),
        code: value.code,
    })
}

fn result_name<'a>(field: &Field, result: &'a Value) -> Option<&'a str> {
    field
        .result_path
        .as_ref()
        .and_then(|path| result.pointer(path))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
}

/// The name from a child's canonical spawn call or frozen role metadata.
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
            Part::ToolCall { arguments, .. } => arguments.get("title")?.as_str(),
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

fn session_title<'a>(value: &str, page: &'a Page) -> Option<Cow<'a, str>> {
    let session = value.parse::<SessionId>().ok()?;
    let child = page
        .children
        .iter()
        .filter(|child| child.run.session == session)
        .max_by_key(|child| child.run.sequence)?;
    child_title(Some(page), child)
}

pub(super) fn resolve(call: &Call, page: &Page) -> Option<Resolved> {
    let display = call.display(page);
    let arguments = call.arguments(page).unwrap_or(&Value::Null);
    let result = call.result(page).unwrap_or(&Value::Null);
    let metadata = ResultDisplay::from_value(result);
    if display.is_none() && metadata.is_none() {
        return None;
    }
    let input = display.and_then(|display| display.input.as_ref());
    let approval = display
        .into_iter()
        .flat_map(|display| &display.approval)
        .filter_map(|prompt| {
            Some(LocalText {
                label: prompt.render(arguments, "")?,
                locales: prompt
                    .message
                    .locales
                    .keys()
                    .filter_map(|locale| Some((locale.clone(), prompt.render(arguments, locale)?)))
                    .collect(),
            })
        })
        .collect();
    let output = display
        .and_then(|display| display.output.as_ref())
        .map(|output| {
            let mut paths = Vec::new();
            if let Some(source) = &output.paths {
                for path in source.read(arguments, result) {
                    if !paths.iter().any(|existing| existing == path) {
                        paths.push(path.to_owned());
                    }
                }
            }
            Output {
                preview: output
                    .preview
                    .as_ref()
                    .filter(|text| !text.deferred())
                    .map(|text| text.render(arguments, result))
                    .unwrap_or_default(),
                preview_deferred: output
                    .preview
                    .as_ref()
                    .is_some_and(|text| text.deferred() && text.visible(arguments, result)),
                notices: output
                    .notices
                    .iter()
                    .filter(|notice| {
                        notice
                            .when
                            .as_ref()
                            .is_none_or(|test| test.matches(arguments, result))
                    })
                    .map(|notice| label(&notice.message))
                    .collect(),
                paths,
                table: output
                    .table
                    .as_ref()
                    .and_then(|table| table.render(arguments, result)),
                body: output.body.as_ref().and_then(|body| {
                    Some(Body {
                        text: body.text.read(arguments, result)?.as_str()?.to_owned(),
                        path: body
                            .path
                            .as_ref()
                            .and_then(|path| path.read(arguments, result)?.as_str())
                            .filter(|path| !path.is_empty())
                            .map(str::to_owned),
                        diff: body
                            .diff
                            .as_ref()
                            .filter(|diff| {
                                diff.when
                                    .as_ref()
                                    .is_none_or(|test| test.matches(arguments, result))
                            })
                            .map(|diff| diff.format),
                        empty: body.empty.as_ref().map(label),
                    })
                }),
            }
        });
    Some(Resolved {
        label: display.map(|display| LocalText {
            label: display.label.clone(),
            locales: display.locales.clone(),
        }),
        input: Input {
            summary: field(input.and_then(|input| input.summary.as_ref()), arguments)
                .or_else(|| {
                    let field = input.and_then(|input| input.summary.as_ref())?;
                    Some(Text {
                        text: result_name(field, result)?.to_owned(),
                        code: field.code,
                    })
                })
                .and_then(|mut text| {
                    let field = input.and_then(|input| input.summary.as_ref())?;
                    if field.format == Some(sailry_protocol::tool::InputFormat::Connection)
                        && let Some(name) = field.names.get(&text.text)
                        && !name.trim().is_empty()
                    {
                        text.text = name.clone();
                    }
                    if let Some(name) = result_name(field, result) {
                        text.text = name.to_owned();
                    }
                    match field.format {
                        Some(sailry_protocol::tool::InputFormat::Command) => {
                            text.text = command::format(&text.text, &display.unwrap().aliases);
                        }
                        Some(sailry_protocol::tool::InputFormat::Session) => {
                            text.text = session_title(&text.text, page)
                                .map(Cow::into_owned)
                                .or_else(|| {
                                    field
                                        .names
                                        .get(&text.text)
                                        .filter(|name| !name.trim().is_empty())
                                        .cloned()
                                })?;
                        }
                        Some(sailry_protocol::tool::InputFormat::Connection) | None => {}
                    }
                    Some(text)
                }),
            context: field(input.and_then(|input| input.context.as_ref()), arguments),
            target: field(input.and_then(|input| input.target.as_ref()), arguments),
            content: field(input.and_then(|input| input.content.as_ref()), arguments),
        },
        output,
        approval,
        status: metadata.as_ref().and_then(|display| display.status.clone()),
        diagnostics: metadata
            .map(|display| display.diagnostics)
            .unwrap_or_default(),
    })
}
