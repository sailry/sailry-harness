use super::server::Server;
use sailry_protocol::{Fault, code};
use serde_json::Value;

pub(super) async fn completions(
    server: &Server,
    path: &str,
    value: Value,
    partial: &mut bool,
) -> Result<code::Content, Fault> {
    *partial |= value
        .get("isIncomplete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    *partial |= items.len() > 100;
    let documents = server.documents.lock().await;
    let text = &documents.0[path].text.text;
    let mut completions = Vec::new();
    for item in items.into_iter().take(100) {
        if item
            .get("command")
            .is_some_and(|command| !command.is_null())
            || item.get("additionalTextEdits").is_some_and(|edits| {
                !edits.is_null() && edits.as_array().is_none_or(|edits| !edits.is_empty())
            })
            || item.get("insertTextFormat").and_then(Value::as_u64) == Some(2)
        {
            *partial = true;
            continue;
        }
        let Some(label) = item
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| label.len() <= 4096 && !label.contains('\0'))
        else {
            continue;
        };
        let edit = item.get("textEdit");
        let insertion = edit
            .and_then(|edit| edit.get("newText"))
            .or_else(|| item.get("insertText"))
            .and_then(Value::as_str)
            .unwrap_or(label);
        if insertion.len() > 8192 || insertion.contains('\0') {
            *partial = true;
            continue;
        }
        let range = if let Some(edit) = edit {
            let range = edit.get("range").or_else(|| edit.get("replace"));
            let Some(range) = range else {
                *partial = true;
                continue;
            };
            let Ok(range) = serde_json::from_value::<lsp_types::Range>(range.clone()) else {
                *partial = true;
                continue;
            };
            let (Some(start), Some(end)) = (
                server.encoding.display(text, range.start),
                server.encoding.display(text, range.end),
            ) else {
                *partial = true;
                continue;
            };
            if (start.line, start.column) > (end.line, end.column) {
                *partial = true;
                continue;
            }
            Some(code::Location {
                path: path.into(),
                start,
                end,
            })
        } else {
            None
        };
        let field = |name: &str| max_text(item.get(name).and_then(Value::as_str), 2048);
        completions.push(code::Completion {
            label: label.into(),
            detail: field("detail"),
            insert_text: insertion.into(),
            range,
            kind: item
                .get("kind")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            sort_text: field("sortText"),
            filter_text: field("filterText"),
        });
    }
    Ok(code::Content::Completions(completions))
}
fn max_text(value: Option<&str>, limit: usize) -> Option<String> {
    value
        .filter(|value| value.len() <= limit && !value.contains('\0'))
        .map(str::to_owned)
}

#[cfg(all(test, unix))]
mod tests;
