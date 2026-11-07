use super::terminal::normalize;
use super::*;

const STREAM_BYTES: usize = 16 * 1024;
const STREAM_LINES: usize = (MAX_LINES - 8) / 2;

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
        let captured = preview["tail"]
            .as_str()
            .or_else(|| data[stream]["text"].as_str());
        let Some(captured) = captured else {
            continue;
        };
        let (start, rendered) = tail(captured);
        let has_tail = preview["tail"].is_string();
        if !has_tail && start == 0 {
            if rendered != captured {
                data[stream]["text"] = json!(rendered);
                changed = true;
            }
            continue;
        }
        let invalid = if has_tail {
            preview["invalid_utf8"] == true
        } else {
            data[stream]["invalid_utf8"] == true
        };
        let captured_bytes = captured.len() - start;
        let captured_lines = limit::count(&rendered);
        let total = preview["total_bytes"]
            .as_u64()
            .or_else(|| data[stream]["total_bytes"].as_u64())
            .or_else(|| (data[stream]["truncated"] == false).then_some(captured.len() as u64));
        let total_label = total.map_or_else(
            || "total bytes unavailable".to_owned(),
            |bytes| format!("{bytes} raw bytes total"),
        );
        let offset = preview["tail_offset"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(start as u64);
        let retained = preview["retained_bytes"].as_u64();
        let retained_end = preview["retained_end"].as_u64().or(retained);
        let retained_tail = preview["retained_tail_offset"].as_u64();
        let raw_offset = preview["transcoded"] != true || has_tail && start == 0;
        let readable_tail = !invalid && raw_offset && retained_end.is_none_or(|end| offset < end);
        let hint = super::hint(original, Some(stream), Some(0));
        let location = if readable_tail {
            format!("; preview tail starts at offset={offset}")
        } else {
            String::new()
        };
        let retention = retained_end.map_or(String::new(), |end| {
            format!("; retained output ends at offset={end}")
        });
        data[stream] = json!({
            "text":format!("[Showing last {captured_lines} captured lines ({captured_bytes} captured bytes); {total_label}; {hint}{location}{retention}]\n{rendered}"),
            "truncated":true,
            "invalid_utf8":invalid,
            "captured_bytes":captured_bytes,
            "total_bytes":total,
            "tail_offset":readable_tail.then_some(offset),
            "retained_bytes":retained,
            "retained_tail_offset":retained_tail,
            "retained_end":retained_end,
        });
        if !invalid && raw_offset {
            data[stream]["omitted_bytes"] = json!(offset);
        }
        changed = true;
    }
    if changed {
        if let Some(id) = id(original) {
            result["sailry_command"] = json!({"id":id});
        }
        result["output"] = source.clone();
    }
    changed.then_some(result)
}

fn tail(text: &str) -> (usize, String) {
    let mut bytes = STREAM_BYTES;
    loop {
        let start = limit::tail_start(text, bytes, STREAM_LINES);
        let rendered = normalize(&text[start..]);
        if serde_json::to_string(&rendered)
            .expect("command text is JSON")
            .len()
            <= 20 * 1024
        {
            return (start, rendered);
        }
        bytes /= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_only_known_raw_offsets() {
        for text in ["版本".to_owned(), "版本\n".repeat(5000)] {
            let original = json!({
                "stdout":{"text":text,"truncated":true,"invalid_utf8":false},
                "sailry_command":{
                    "id":"fixture",
                    "stdout":{
                        "tail":text,"tail_offset":4096,"total_bytes":40_000,
                        "retained_end":40_000,"transcoded":true,"invalid_utf8":false,
                    },
                },
            });
            let projected = project(&original, &json!({"call":"fixture"})).unwrap();
            let stream = &projected["stdout"];
            assert_eq!(stream["total_bytes"], 40_000);
            if text == "版本" {
                assert_eq!(stream["tail_offset"], 4096);
                assert_eq!(stream["omitted_bytes"], 4096);
            } else {
                assert!(stream["tail_offset"].is_null());
                assert!(stream.get("omitted_bytes").is_none());
                assert!(!stream["text"].as_str().unwrap().contains("tail starts at"));
            }
        }
    }
}
