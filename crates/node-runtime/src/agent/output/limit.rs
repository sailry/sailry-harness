//! Bounds only outgoing tool text. Canonical data stays available through the
//! existing scoped `read_tool_output` service, including after compaction.
use super::*;
use rmcp::model::ResourceContents;

pub(super) fn bound(result: &mut CallToolResult, source: &Value, tail: bool) {
    let (bytes, lines) = size(result);
    if bytes <= MAX_BYTES && lines <= MAX_LINES {
        return;
    }
    let prefix = !tail
        && result.structured_content.as_ref().is_some_and(|value| {
            let plain = CallToolResult::structured(value.clone());
            result.is_error == plain.is_error
                && result.result_type == plain.result_type
                && result.content == plain.content
                && source["revision"].as_str()
                    == Some(blake3::hash(value.to_string().as_bytes()).to_hex().as_str())
        });
    if let Some(value) = result.structured_content.take() {
        let encoded = value.to_string();
        if !result.content.iter().filter_map(text).any(|text| {
            text == encoded || serde_json::from_str::<Value>(text).is_ok_and(|item| item == value)
        }) {
            result.content.push(ContentBlock::text(encoded));
        }
    }
    let (total_bytes, total_lines) = size(result);
    if total_bytes <= MAX_BYTES && total_lines <= MAX_LINES {
        return;
    }
    let reserve = notice(
        source,
        tail,
        total_bytes,
        total_lines,
        total_bytes,
        total_lines,
        total_bytes,
    )
    .len()
        + result.content.iter().filter_map(text).count();
    let mut bytes = MAX_BYTES.saturating_sub(reserve);
    let mut lines = MAX_LINES - 1;
    let (mut shown_bytes, mut shown_lines) = (0, 0);
    let mut indices: Vec<_> = (0..result.content.len()).collect();
    if tail {
        indices.reverse();
    }
    let mut cut = false;
    for index in indices {
        let Some(text) = text_mut(&mut result.content[index]) else {
            continue;
        };
        if cut {
            text.clear();
            continue;
        }
        let original_bytes = text.len();
        let (start, end) = if tail {
            (tail_start(text, bytes, lines), text.len())
        } else {
            (0, head(text, bytes, lines))
        };
        cut = end - start < original_bytes;
        let used = count(&text[start..end]);
        bytes -= end - start;
        lines -= used;
        shown_bytes += end - start;
        shown_lines += used;
        *text = text[start..end].into();
    }
    result
        .content
        .retain(|content| text(content).is_none_or(|text| !text.is_empty()));
    result.content.push(ContentBlock::text(notice(
        source,
        tail,
        total_bytes,
        total_lines,
        shown_bytes,
        shown_lines,
        if prefix { shown_bytes } else { 0 },
    )));
}

pub(super) fn error(call: &str, error: &mut rmcp::model::ErrorData, saved: Option<&Value>) {
    let data = error
        .data
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_default();
    if error.message.len() + data.len() <= MAX_BYTES && count(&error.message) <= MAX_LINES {
        return;
    }
    let revision = saved
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            blake3::hash(
                serde_json::to_value(&*error)
                    .expect("tool error is JSON")
                    .to_string()
                    .as_bytes(),
            )
            .to_hex()
            .to_string()
        });
    let source = json!({"call":call,"revision":revision});
    let mut result = CallToolResult::success(vec![ContentBlock::text(format!(
        "{}\n{data}",
        error.message
    ))]);
    bound(&mut result, &source, false);
    error.message = result
        .content
        .iter()
        .filter_map(text)
        .collect::<Vec<_>>()
        .join("\n")
        .into();
    error.data = None;
}

fn notice(
    source: &Value,
    tail: bool,
    total_bytes: usize,
    total_lines: usize,
    shown_bytes: usize,
    shown_lines: usize,
    offset: usize,
) -> String {
    let edge = if tail { "last" } else { "first" };
    format!(
        "[Tool result truncated: showing the {edge} {shown_lines} of {total_lines} lines ({shown_bytes} of {total_bytes} bytes). Read the original result with read_tool_output using output={source} and offset={offset}, then continue with next_offset]"
    )
}

fn size(result: &CallToolResult) -> (usize, usize) {
    let (mut bytes, mut lines) = result
        .content
        .iter()
        .filter_map(text)
        .fold((0, 0), |(bytes, lines), text| {
            (bytes + text.len(), lines + count(text))
        });
    // Plugin providers may serialize structuredContent as well as content.
    if let Some(value) = &result.structured_content {
        let value = value.to_string();
        bytes += value.len();
        lines += count(&value);
    }
    (bytes, lines)
}

pub(super) fn text(content: &ContentBlock) -> Option<&str> {
    match content {
        ContentBlock::Text(text) => Some(&text.text),
        ContentBlock::Resource(resource) => match &resource.resource {
            ResourceContents::TextResourceContents { text, .. } => Some(text),
            _ => None,
        },
        _ => None,
    }
}

fn text_mut(content: &mut ContentBlock) -> Option<&mut String> {
    match content {
        ContentBlock::Text(text) => Some(&mut text.text),
        ContentBlock::Resource(resource) => match &mut resource.resource {
            ResourceContents::TextResourceContents { text, .. } => Some(text),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn count(text: &str) -> usize {
    text.matches('\n').count() + usize::from(!text.is_empty() && !text.ends_with('\n'))
}

pub(super) fn head(text: &str, bytes: usize, lines: usize) -> usize {
    if lines == 0 {
        return 0;
    }
    let (mut end, _) = crate::process::output::edges(text.as_bytes(), bytes, 0);
    if let Some((index, _)) = text[..end].match_indices('\n').nth(lines - 1) {
        end = index + 1;
    } else if end < text.len()
        && let Some(index) = text[..end].rfind('\n')
    {
        end = index + 1;
    }
    end
}

pub(super) fn tail_start(text: &str, bytes: usize, lines: usize) -> usize {
    if lines == 0 {
        return text.len();
    }
    let (_, mut start) = crate::process::output::edges(text.as_bytes(), 0, bytes);
    // A final newline terminates the last line; it does not start an empty one.
    if let Some((index, _)) = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .rmatch_indices('\n')
        .nth(lines - 1)
    {
        start = start.max(index + 1);
    }
    start
}

pub(super) fn reference(call: &str, result: &CallToolResult, saved: Option<&Value>) -> Value {
    let revision = saved
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let canonical = match &result.structured_content {
                Some(value)
                    if {
                        let plain = CallToolResult::structured(value.clone());
                        result.is_error == plain.is_error
                            && result.result_type == plain.result_type
                            && result.content == plain.content
                    } =>
                {
                    value.clone()
                }
                _ => {
                    let mut original = result.clone();
                    if let Some(meta) = original.meta.as_mut() {
                        meta.remove("sailry_context");
                    }
                    serde_json::to_value(original).expect("tool result is JSON")
                }
            };
            blake3::hash(canonical.to_string().as_bytes())
                .to_hex()
                .to_string()
        });
    json!({"call":call,"revision":revision})
}

#[cfg(test)]
mod tests;
