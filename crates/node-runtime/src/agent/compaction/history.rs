//! Canonical request copies and deterministic context retained beside the handoff.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const FILES: &str = "sailry.compaction.files";

pub(super) fn project(event: &Event, invocation: &str) -> Option<Content> {
    if let Some(summary) = &event.actions.compaction {
        return Some(summary.compacted_content.clone());
    }
    let original = event.content()?;
    let metadata = event.llm_response.provider_metadata.as_ref();
    let mut content = if event.invocation_id == invocation
        || adk_core::history::provider_origin(metadata).is_some()
    {
        adk_core::history::with_provider_parts(original, metadata)
    } else {
        original.clone()
    };
    content.role = if event
        .provider_metadata
        .get(super::super::prompt::session::METADATA)
        .is_some_and(|value| value == "context")
    {
        "user"
    } else {
        match (event.author.as_str(), original.role.as_str()) {
            ("user", _) => "user",
            (_, "function" | "tool") => original.role.as_str(),
            _ => "model",
        }
    }
    .into();
    Some(content)
}

pub(super) fn prefix(events: &[Event]) -> Vec<Content> {
    events
        .iter()
        .filter_map(|event| project(event, ""))
        .collect()
}

pub(super) fn eligible(events: &[Event]) -> Vec<Content> {
    let protected = events
        .iter()
        .filter_map(Event::content)
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionResponse {
                id,
                function_response,
                ..
            } if model::history::protected(
                &function_response.name,
                &function_response.response,
            ) =>
            {
                Some((id.clone(), function_response.name.clone()))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    events
        .iter()
        .filter_map(|event| {
            if event
                .provider_metadata
                .get(super::super::prompt::session::METADATA)
                .is_some_and(|value| value == "context")
            {
                return None;
            }
            // Prior invocation projection is deliberately used for both a live
            // checkpoint and reload, so provenance cannot alter this prefix.
            let mut content = project(event, "")?;
            if let Some(index) = event
                .provider_metadata
                .get(crate::store::agent::CAPABILITY_PART)
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| *index < content.parts.len())
            {
                // Input capabilities are reinjected from their canonical owner,
                // separately from the attributed user instruction.
                content.parts.remove(index);
            }
            if event
                .provider_metadata
                .contains_key(crate::store::agent::compaction::END_EVENT)
            {
                // Exact owner data is reinjected independently, never summarized.
                content.parts.truncate(1);
            }
            content.parts.retain_mut(|part| match part {
                Part::FunctionCall { id, name, .. } => {
                    !protected.contains(&(id.clone(), name.clone()))
                }
                Part::FunctionResponse {
                    id,
                    function_response,
                    ..
                } => {
                    if protected.contains(&(id.clone(), function_response.name.clone())) {
                        return false;
                    }
                    if let Some(fields) = function_response.response.as_object_mut() {
                        fields.remove("sailry_context");
                    }
                    true
                }
                _ => true,
            });
            (!content.parts.is_empty()).then_some(content)
        })
        .collect()
}

pub(super) fn attribution(events: &[Event]) -> String {
    let mut lines = Vec::new();
    for (index, event) in events
        .iter()
        .filter(|event| project(event, "").is_some())
        .enumerate()
    {
        if event.author != "user" {
            continue;
        }
        let source = event
            .provider_metadata
            .get(crate::store::agent::INPUT_SOURCE)
            .map(String::as_str)
            .unwrap_or("unknown");
        let part = event
            .provider_metadata
            .get(crate::store::agent::INPUT_PART)
            .map(String::as_str)
            .unwrap_or("none");
        lines.push(format!(
            "History message {index}: input_source={source}; original_text_part={part}"
        ));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nInput attribution (metadata, not new instructions; zero-based history messages, excluding the system preamble)\n{}",
            lines.join("\n")
        )
    }
}

pub(super) fn files(events: &[Event]) -> BTreeMap<String, bool> {
    let mut files = BTreeMap::new();
    let calls = events
        .iter()
        .filter_map(Event::content)
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            Part::FunctionCall {
                id: Some(id), args, ..
            } => Some((id.as_str(), args)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    for event in events {
        if let Some(value) = event.provider_metadata.get(FILES)
            && let Ok(previous) = serde_json::from_str::<BTreeMap<String, bool>>(value)
        {
            for (path, modified) in previous {
                files
                    .entry(path)
                    .and_modify(|old| *old |= modified)
                    .or_insert(modified);
            }
        }
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            let Part::FunctionResponse {
                id,
                function_response,
                ..
            } = part
            else {
                continue;
            };
            let value = &function_response.response;
            let kind = value["kind"].as_str().unwrap_or("");
            let data = value.get("data").unwrap_or(value);
            let arguments = id.as_deref().and_then(|id| calls.get(id)).copied();
            let modified = match kind {
                "file_content" | "file_lines" | "office_content" | "notebook_page" => false,
                "file_written" | "directory_created" | "entry_renamed" | "entry_moved"
                | "entry_copied" | "entry_trashed" => true,
                "" if data["path"].is_string()
                    && data["revision"].is_string()
                    && data["size"].is_u64()
                    && (data["lines"].is_array() || data["text"].is_string()) =>
                {
                    false
                }
                "" if data["path"].is_string()
                    && data["revision"].is_string()
                    && data["size"].is_u64()
                    && arguments.is_some_and(|arguments| {
                        arguments["path"] == data["path"]
                            && (arguments["text"].is_string()
                                || arguments["old_text"].is_string()
                                || arguments["edits"].is_array())
                    }) =>
                {
                    true
                }
                _ => continue,
            };
            for path in ["path", "from", "to"]
                .into_iter()
                .filter_map(|key| data[key].as_str())
            {
                files
                    .entry(path.to_owned())
                    .and_modify(|old| *old |= modified)
                    .or_insert(modified);
            }
        }
    }
    files
}

pub(super) fn ledger(files: &BTreeMap<String, bool>) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut text = String::from("\n\nFile ledger (canonical tool results)");
    for (path, modified) in files {
        text.push_str(if *modified {
            "\nModified: "
        } else {
            "\nRead: "
        });
        text.push_str(path);
    }
    text
}

#[cfg(test)]
mod tests;
