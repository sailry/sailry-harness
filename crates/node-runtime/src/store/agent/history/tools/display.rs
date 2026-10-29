//! Captured finite recipes resolve on the execution Node, before transport.
use sailry_protocol::{
    plugin::desktop::Navigation,
    tool::{
        Display, Field, InputFormat, LocalText, ResultDisplay,
        detail::{Body, Input, Output, Resolved, Text},
    },
};
use serde::Deserialize;
use serde_json::Value;
mod command;

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

pub(super) fn resolve(
    display: Option<&Display>,
    arguments: &Value,
    result: &Value,
    full: bool,
) -> Option<Resolved> {
    let display = display.filter(|display| display.valid());
    let metadata = ResultDisplay::from_value(result);
    if display.is_none() && metadata.is_none() {
        return None;
    }
    let input = display.and_then(|display| display.input.as_ref());
    let command = input
        .and_then(|input| input.summary.as_ref())
        .filter(|summary| summary.format == Some(InputFormat::Command))
        .and_then(
            |_| match sailry_protocol::Output::deserialize(result).ok()? {
                sailry_protocol::Output::CommandResult(completion) => Some(completion.outcome),
                _ => None,
            },
        );
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
            let mut unique = std::collections::BTreeSet::new();
            if let Some(source) = &output.paths {
                for path in source.read(arguments, result) {
                    if unique.insert(path) && (full || paths.len() < 32) {
                        paths.push(path.to_owned());
                    }
                }
            }
            Output {
                // Presence of a preview recipe is the explicit visibility declaration.
                preview: output
                    .preview
                    .as_ref()
                    .filter(|text| full || !text.deferred())
                    .map(|text| text.render(arguments, result))
                    .unwrap_or_default(),
                preview_deferred: !full
                    && output
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
                path_count: unique.len(),
                table: full
                    .then(|| {
                        output
                            .table
                            .as_ref()
                            .and_then(|table| table.render(arguments, result))
                    })
                    .flatten(),
                body: full
                    .then(|| {
                        let body = output.body.as_ref()?;
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
                    })
                    .flatten(),
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
                    let name = result_name(field, result);
                    if let Some(name) = name {
                        text.text = name.to_owned();
                    }
                    match field.format {
                        Some(InputFormat::Command) => {
                            text.text = command::format(&text.text, &display.unwrap().aliases);
                        }
                        Some(InputFormat::Session) if name.is_none() => {
                            text.text = field
                                .names
                                .get(&text.text)
                                .filter(|name| !name.trim().is_empty())
                                .cloned()?;
                        }
                        Some(InputFormat::Connection) if name.is_none() => {
                            if let Some(name) = field
                                .names
                                .get(&text.text)
                                .filter(|name| !name.trim().is_empty())
                            {
                                text.text = name.clone();
                            }
                        }
                        _ => {}
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
        command,
        diagnostics: metadata
            .map(|display| display.diagnostics)
            .unwrap_or_default(),
    })
}
