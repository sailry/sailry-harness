//! Full file revisions shown to the model by its own file tools. They come from
//! the model-visible tool results of the session the tool runs in, so a rewound,
//! forked or compacted conversation keeps only what the model can still see. Only
//! results of tools bound to Node file operations count; other tools may return
//! lookalike values.
use crate::agent::model::{Content, Message};
use sailry_protocol::tool::Operation;
use serde_json::Value;
use std::collections::HashMap;

/// Distinct paths retained for one call; older observations require another read.
const PATHS: usize = 4096;

#[derive(Debug, Default)]
pub(in crate::agent) struct Observed(HashMap<String, Option<String>>);

impl Observed {
    /// Operations whose results report the files they read or wrote.
    pub(in crate::agent) fn source(operation: Operation) -> bool {
        matches!(
            operation,
            Operation::ReadFile
                | Operation::ReadFileLines
                | Operation::WriteFile
                | Operation::EditFile
                | Operation::ApplyChanges
                | Operation::DeleteFile
                | Operation::MoveFile
                | Operation::ReadNotebook
                | Operation::EditNotebook
        )
    }

    /// The newest result for each path wins. A move, deletion or uncertain outcome
    /// is recorded without a revision. `source` names the tools whose results count.
    pub(in crate::agent) fn collect(
        conversation: Option<&[Message]>,
        source: impl Fn(&str) -> bool,
    ) -> Self {
        let mut observed = HashMap::new();
        let messages = conversation.unwrap_or_default();
        let calls: HashMap<&str, &str> = messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|content| match content {
                Content::ToolCall(request) if request.error.is_none() => {
                    Some((request.id.as_str(), request.name.as_str()))
                }
                _ => None,
            })
            .collect();
        'messages: for message in messages.iter().rev() {
            if !message.is_agent_visible() {
                continue;
            }
            for content in message.content.iter().rev() {
                let Content::ToolResult(response) = content else {
                    continue;
                };
                if !calls
                    .get(response.id.as_str())
                    .is_some_and(|name| source(name))
                {
                    continue;
                }
                let Ok(result) = &response.result else {
                    continue;
                };
                let Some(value) = result
                    .structured_content
                    .as_ref()
                    .filter(|_| result.is_error != Some(true))
                else {
                    continue;
                };
                for (path, revision) in facts(value).into_iter().rev() {
                    if observed.len() >= PATHS && !observed.contains_key(&path) {
                        break 'messages;
                    }
                    observed.entry(path).or_insert(revision);
                }
            }
        }
        Self(observed)
    }

    pub(in crate::agent) fn revision(&self, path: &str) -> Option<&str> {
        self.0.get(path)?.as_deref()
    }
}

/// Paths and revisions established by one Node file output, in result order.
fn facts(value: &Value) -> Vec<(String, Option<String>)> {
    let data = &value["data"];
    let text = |value: &Value| value.as_str().map(str::to_owned);
    match value["kind"].as_str() {
        Some("file_lines" | "file_image" | "file_content" | "file_written" | "notebook_page") => {
            text(&data["path"])
                .map(|path| vec![(path, text(&data["revision"]))])
                .unwrap_or_default()
        }
        Some("file_changes") => data["files"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|file| {
                let path = text(&file["path"]);
                let destination = text(&file["destination"]);
                let revision = text(&file["revision"]);
                match (file["status"].as_str(), file["kind"].as_str()) {
                    (Some("applied"), Some("added" | "modified")) => vec![(path, revision)],
                    (Some("applied"), Some("renamed")) => {
                        vec![(path, None), (destination, revision)]
                    }
                    (Some("applied"), Some("deleted")) => vec![(path, None)],
                    (Some("unknown"), _) => vec![(path, None), (destination, None)],
                    _ => Vec::new(),
                }
            })
            .filter_map(|(path, revision)| Some((path?, revision)))
            .collect(),
        Some("entry_trashed") => text(&data["path"])
            .map(|path| vec![(path, None)])
            .unwrap_or_default(),
        Some("entry_renamed") => [text(&data["from"]), text(&data["to"])]
            .into_iter()
            .flatten()
            .map(|path| (path, None))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolRequestParams, CallToolResult};
    use serde_json::json;

    fn call(id: &str, name: &'static str) -> Message {
        Message::assistant()
            .with_generated_id()
            .with_tool_request(id, Ok(CallToolRequestParams::new(name)))
    }

    fn result(id: &str, value: Value) -> Message {
        Message::user()
            .with_generated_id()
            .with_tool_response(id, Ok(CallToolResult::structured(value)))
    }

    /// Each result follows a call of the files tool unless a test names another tool.
    fn observe(messages: Vec<Message>) -> Observed {
        let messages = messages
            .into_iter()
            .flat_map(|message| {
                let id = message.content.iter().find_map(|content| match content {
                    Content::ToolResult(response) => Some(response.id.clone()),
                    _ => None,
                });
                id.map(|id| call(&id, "read_file"))
                    .into_iter()
                    .chain([message])
            })
            .collect::<Vec<_>>();
        Observed::collect(Some(&messages), |name| name == "read_file")
    }

    fn revision(text: &str) -> String {
        blake3::hash(text.as_bytes()).to_hex().to_string()
    }

    #[test]
    fn keeps_newest_visible_result() {
        let messages = vec![
            result(
                "read",
                json!({"kind":"file_lines","data":{"path":"src/lib.rs","revision":revision("old"),"lines":[]}}),
            ),
            result(
                "edit",
                json!({"kind":"file_written","data":{"path":"src/lib.rs","revision":revision("new"),"size":3}}),
            ),
            result(
                "apply",
                json!({"kind":"file_changes","data":{"status":"partial","files":[
                    {"path":"a.rs","kind":"added","status":"applied","revision":revision("a")},
                    {"path":"b.rs","destination":"c.rs","kind":"renamed","status":"applied","revision":revision("b")},
                    {"path":"d.rs","kind":"modified","status":"unknown"},
                    {"path":"e.rs","kind":"modified","status":"not_attempted"}
                ]}}),
            ),
            result(
                "hidden",
                json!({"kind":"file_lines","data":{"path":"hidden.rs","revision":revision("hidden")}}),
            )
            .with_visibility(true, false),
            result(
                "notebook",
                json!({"kind":"notebook_page","data":{"path":"a.ipynb","revision":revision("nb"),"cells":[]}}),
            ),
            result(
                "image",
                json!({"kind":"file_image","data":{"path":"logo.png","revision":revision("png")}}),
            ),
        ];
        let observed = observe(messages);
        assert_eq!(observed.revision("a.ipynb"), Some(revision("nb").as_str()));
        assert_eq!(
            observed.revision("logo.png"),
            Some(revision("png").as_str())
        );
        assert_eq!(
            observed.revision("src/lib.rs"),
            Some(revision("new").as_str())
        );
        assert_eq!(observed.revision("a.rs"), Some(revision("a").as_str()));
        assert_eq!(observed.revision("c.rs"), Some(revision("b").as_str()));
        for absent in ["b.rs", "d.rs", "e.rs", "hidden.rs", "missing.rs"] {
            assert_eq!(observed.revision(absent), None, "{absent}");
        }
    }

    #[test]
    fn ignores_failed_and_foreign_results() {
        let mut failed = CallToolResult::structured(
            json!({"kind":"file_lines","data":{"path":"failed.rs","revision":revision("x")}}),
        );
        failed.is_error = Some(true);
        let messages = vec![
            Message::user()
                .with_generated_id()
                .with_tool_response("failed", Ok(failed)),
            result("other", json!({"path":"other.rs","revision":revision("y")})),
            result(
                "trashed",
                json!({"kind":"file_lines","data":{"path":"gone.rs","revision":revision("z")}}),
            ),
            result(
                "removed",
                json!({"kind":"entry_trashed","data":{"path":"gone.rs"}}),
            ),
        ];
        let observed = observe(messages);
        for path in ["failed.rs", "other.rs", "gone.rs"] {
            assert_eq!(observed.revision(path), None, "{path}");
        }
        assert_eq!(
            Observed::collect(None, |_| true).revision("failed.rs"),
            None
        );
    }

    #[test]
    fn ignores_lookalike_tools() {
        let lookalike =
            json!({"kind":"file_lines","data":{"path":"mcp.rs","revision":revision("mcp")}});
        let messages = vec![
            call("mcp", "mcp__files__read"),
            result("mcp", lookalike.clone()),
            result("orphan", lookalike),
        ];
        let observed = Observed::collect(Some(&messages), |name| name == "read_file");
        assert_eq!(observed.revision("mcp.rs"), None);
    }
}
