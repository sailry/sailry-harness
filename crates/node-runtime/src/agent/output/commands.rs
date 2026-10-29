use super::terminal::normalize;
use super::*;

const STREAM_EDGE: usize = 8 * 1024;

pub(super) use crate::process::output::command_id as id;

pub(super) fn project(original: &Value, source: &Value) -> Option<Value> {
    let mut changed = false;
    let mut result = original.clone();
    let wrapped = original.get("data").is_some();
    for stream in ["stdout", "stderr"] {
        let data = if wrapped {
            result.get_mut("data")?
        } else {
            &mut result
        };
        let preview = &original["sailry_command"][stream];
        if let (Some(tail), Some(total), Some(tail_offset)) = (
            preview["tail"].as_str(),
            preview["total_bytes"].as_u64(),
            preview["tail_offset"].as_u64(),
        ) {
            let captured = preview["head"]
                .as_str()
                .or_else(|| data[stream]["text"].as_str())
                .unwrap_or("");
            let invalid_head = if preview["head"].is_string() {
                preview["head_invalid_utf8"] == true
            } else {
                data[stream]["invalid_utf8"] == true
            };
            let (head_bytes, _) =
                crate::process::output::edges(captured.as_bytes(), STREAM_EDGE, 0);
            let head = &captured[..head_bytes];
            let omitted = tail_offset.saturating_sub(head_bytes as u64);
            let retained = preview["retained_bytes"].as_u64().unwrap_or(0);
            let retained_tail = preview["retained_tail_offset"].as_u64().unwrap_or(0);
            let retained_end = preview["retained_end"].as_u64().unwrap_or(retained);
            let hint = super::hint(
                original,
                Some(stream),
                Some(if invalid_head || head_bytes as u64 >= retained_end {
                    0
                } else {
                    head_bytes as u64
                }),
            );
            let omission = if invalid_head {
                format!("captured output omitted; {total} raw bytes total")
            } else {
                format!("{omitted} of {total} raw bytes omitted")
            };
            let readable_tail = tail_offset < retained_end;
            let tail_location = if readable_tail {
                format!("; preview tail starts at offset={tail_offset}")
            } else {
                String::new()
            };
            let retained_location = if retained_tail < retained_end {
                format!("; retained tail starts at offset={retained_tail}")
            } else {
                String::new()
            };
            data[stream] = json!({
                "text":format!("{}\n[... {omission}; {hint}{tail_location}{retained_location}; retained output ends at offset={retained_end} ...]\n{}", normalize(head), normalize(tail)),
                "truncated":true,"invalid_utf8":invalid_head || preview["invalid_utf8"] == true,
                "total_bytes":total,
                "tail_offset":readable_tail.then_some(tail_offset),"retained_bytes":retained,"retained_tail_offset":retained_tail,"retained_end":retained_end,
            });
            if !invalid_head {
                data[stream]["omitted_bytes"] = json!(omitted);
                data[stream]["head_bytes"] = json!(head_bytes);
            }
            changed = true;
            continue;
        }
        let Some(text) = data[stream]["text"].as_str() else {
            continue;
        };
        let reduced = normalize(text);
        let replacement = if reduced.len() > STREAM_EDGE * 2 {
            // Measure offsets in the raw capture before terminal normalization.
            let (head, tail) =
                crate::process::output::edges(text.as_bytes(), STREAM_EDGE, STREAM_EDGE);
            let omitted = tail - head;
            let invalid_utf8 = data[stream]["invalid_utf8"] == true;
            let hint = super::hint(
                original,
                Some(stream),
                Some(if invalid_utf8 { 0 } else { head as u64 }),
            );
            let total = text.len();
            let complete = data[stream]["truncated"] == false;
            let omission = if invalid_utf8 {
                format!("captured output omitted; {hint}")
            } else {
                format!(
                    "{omitted} of {total} captured raw bytes omitted; {hint}; tail starts at offset={tail}"
                )
            };
            let projected = format!(
                "{}\n[... {omission} ...]\n{}",
                normalize(&text[..head]),
                normalize(&text[tail..])
            );
            data[stream]["truncated"] = Value::Bool(true);
            if !invalid_utf8 {
                data[stream]["omitted_bytes"] = json!(omitted);
                data[stream]["captured_bytes"] = json!(total);
                if complete {
                    data[stream]["total_bytes"] = json!(total);
                }
                data[stream]["head_bytes"] = json!(head);
                data[stream]["tail_offset"] = json!(tail);
            }
            Some(projected)
        } else {
            (reduced.len() < text.len()).then_some(reduced)
        };
        if let Some(text) = replacement {
            data[stream]["text"] = json!(text);
            changed = true;
        }
    }
    if changed {
        if let Some(id) = id(original) {
            result["sailry_command"] = json!({"id":id});
        }
        result["output"] = source.clone();
    }
    changed.then_some(result)
}
