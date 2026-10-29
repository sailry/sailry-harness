//! Presentation is captured by Node registration, never supplied by model arguments.
use crate::agent::canonical::{BlockKind, Record, ToolFormat, ToolStatus};
use sailry_protocol::tool::Presentation;
use std::collections::BTreeMap;

pub(super) use crate::agent::canonical::ToolFailure as Failure;

pub(super) fn capture_failures(
    event: &mut Record,
    timeline: Option<&crate::agent::observation::Timeline>,
) -> Vec<Failure> {
    let failures: Vec<_> = event
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| {
            let BlockKind::ToolResponse(response) = &block.kind else {
                return None;
            };
            let id = &response.id;
            let observed = timeline.and_then(|timeline| {
                timeline.tool_failure(&event.provenance.branch, id, &response.name)
            });
            let failure = match response.status {
                ToolStatus::Error => {
                    let encoded = (response.format == ToolFormat::McpError)
                        .then(|| response.result.value.get("data"))
                        .flatten()
                        .and_then(|value| {
                            serde_json::from_value::<sailry_protocol::Fault>(value.clone()).ok()
                        })
                        .map(|fault| sailry_protocol::tool::Failure {
                            code: Some(fault.code),
                        });
                    observed
                        .or(encoded)
                        .unwrap_or(sailry_protocol::tool::Failure { code: None })
                }
                ToolStatus::Success => observed?,
            };
            Some(Failure {
                index: block.index,
                id: id.clone(),
                name: response.name.clone(),
                failure,
            })
        })
        .collect();
    event
        .metadata
        .presentation
        .get_or_insert_with(Default::default)
        .failures = failures.clone();
    failures
}

pub(super) fn failures(event: &Record) -> Result<Vec<Failure>, sailry_protocol::Fault> {
    Ok(event
        .metadata
        .presentation
        .as_ref()
        .map(|value| value.failures.clone())
        .unwrap_or_default())
}

pub(super) fn preserve_failures(
    event: &mut Record,
    previous: Option<&[u8]>,
) -> Result<(), sailry_protocol::Fault> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let previous: Record = serde_json::from_slice(previous).map_err(super::storage_error)?;
    if previous.provenance.branch != event.provenance.branch {
        return Ok(());
    }
    let mut captured = failures(event)?;
    for failure in failures(&previous)? {
        if captured
            .iter()
            .any(|current| current.index == failure.index)
        {
            continue;
        }
        if let (Some(old), Some(current)) = (
            response(&previous, failure.index),
            response(event, failure.index),
        ) && old.id == failure.id
            && current.id == old.id
            && old.name == failure.name
            && current.name == failure.name
            && old.revision == current.revision
        {
            captured.push(failure);
        }
    }
    if !captured.is_empty() {
        event
            .metadata
            .presentation
            .get_or_insert_with(Default::default)
            .failures = captured;
    }
    Ok(())
}

fn response(record: &Record, index: usize) -> Option<&crate::agent::canonical::ToolResponse> {
    record
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
        .find(|block| block.index == index)
        .and_then(|block| match &block.kind {
            BlockKind::ToolResponse(response) => Some(response),
            _ => None,
        })
}

pub(super) fn stamp(event: &mut Record, catalog: &crate::agent::canonical::Presentation) {
    let names: std::collections::BTreeSet<_> = event
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match &block.kind {
            BlockKind::ToolRequest(request) => Some(request.name.as_str()),
            _ => None,
        })
        .collect();
    let mut captured = catalog.clone();
    captured
        .tools
        .retain(|name, _| names.contains(name.as_str()));
    captured
        .groupings
        .retain(|name, _| names.contains(name.as_str()));
    captured
        .displays
        .retain(|name, _| names.contains(name.as_str()));
    captured.failures = event
        .metadata
        .presentation
        .as_ref()
        .map(|value| value.failures.clone())
        .unwrap_or_default();
    event.metadata.presentation = Some(captured);
}

pub(super) fn read(
    event: &Record,
) -> Result<BTreeMap<String, Presentation>, sailry_protocol::Fault> {
    Ok(event
        .metadata
        .presentation
        .as_ref()
        .map(|value| value.tools.clone())
        .unwrap_or_default())
}

pub(super) fn groupings(
    event: &Record,
) -> Result<BTreeMap<String, sailry_protocol::tool::Grouping>, sailry_protocol::Fault> {
    Ok(event
        .metadata
        .presentation
        .as_ref()
        .map(|value| value.groupings.clone())
        .unwrap_or_default())
}

pub(super) fn displays(
    event: &Record,
) -> Result<BTreeMap<String, sailry_protocol::tool::Display>, sailry_protocol::Fault> {
    Ok(event
        .metadata
        .presentation
        .as_ref()
        .map(|value| value.displays.clone())
        .unwrap_or_default())
}

/// Capture public child labels with the call, so later history pages need no live lookup.
pub(super) async fn capture_names(
    db: &turso::Connection,
    session: sailry_protocol::SessionId,
    event: &mut Record,
    previous: Option<&[u8]>,
) -> Result<(), sailry_protocol::Fault> {
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
        .map(|body| serde_json::from_slice::<Record>(body).map_err(super::storage_error))
        .transpose()?;
    let original = previous.as_ref().map(displays).transpose()?;
    let source_project = if original.is_none() {
        super::super::commands::session(db, session)
            .await
            .ok()
            .map(|session| session.project)
    } else {
        None
    };
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
        for block in event
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
        {
            let BlockKind::ToolRequest(request) = &block.kind else {
                continue;
            };
            let (tool, args) = (&request.name, &request.arguments);
            if tool != name {
                continue;
            }
            let Some(target) = field
                .read(args)
                .and_then(|value| value.parse::<sailry_protocol::SessionId>().ok())
            else {
                continue;
            };
            if field.names.contains_key(&target.to_string()) {
                continue;
            }
            let child: Option<(Option<String>, String)> = (match (async { let mut statement = db.prepare("SELECT json_extract(body,'$.task.title'),json_extract(body,'$.task.kind.data.turn') FROM agent_background WHERE session=?1 AND kind='agent' AND source=?2 ORDER BY sequence DESC,rowid DESC LIMIT 1").await?;
let row = statement.query_row(turso::params![session.to_string(), target.to_string()]).await?;
let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?,row.get(1)?)))(&row);
mapped }).await { Ok(value) => Ok(Some(value)), Err(turso::Error::QueryReturnedNoRows) => Ok(None), Err(error) => Err(error) }).map_err(super::storage_error)?;
            let title = if let Some((Some(title), turn)) = child {
                let role =
                    super::delegation::child(db, turn.parse().map_err(super::storage_error)?)
                        .await?
                        .and_then(|child| child.role)
                        .map(|role| role.name)
                        .filter(|name| !name.trim().is_empty());
                Some(match role {
                    Some(name) => format!("{name} {title}"),
                    None => title,
                })
            } else {
                None
            };
            let title = if title.is_some() {
                title
            } else if let Some(project) = source_project {
                super::references::history::visible(db, project, target)
                    .await
                    .ok()
                    .map(|target| target.activity.title)
            } else {
                None
            };
            if let Some(title) = title
                .filter(|title| !title.trim().is_empty() && !title.chars().any(char::is_control))
            {
                field.names.insert(target.to_string(), title);
            }
        }
    }
    event
        .metadata
        .presentation
        .get_or_insert_with(Default::default)
        .displays = captured;
    Ok(())
}

#[cfg(test)]
mod tests;
