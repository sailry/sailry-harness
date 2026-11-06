//! Full file revisions shown to the model by its own file tools. They come from
//! the model-visible tool results of the session the tool runs in, so a rewound,
//! forked or compacted conversation keeps only what the model can still see.
use goose_provider_types::conversation::{Conversation, message::MessageContent};
use serde_json::Value;
use std::collections::HashMap;

/// Distinct paths retained for one call; older observations require another read.
const PATHS: usize = 4096;

#[derive(Debug, Default)]
pub(in crate::agent) struct Observed(HashMap<String, Option<String>>);

impl Observed {
    /// The newest result for each path wins. A move, deletion or uncertain outcome
    /// is recorded without a revision.
    pub(in crate::agent) fn collect(conversation: Option<&Conversation>) -> Self {
        let mut observed = HashMap::new();
        let messages =
            conversation.map_or(&[][..], |conversation| conversation.messages().as_slice());
        'messages: for message in messages.iter().rev() {
            if !message.is_agent_visible() {
                continue;
            }
            for content in message.content.iter().rev() {
                let MessageContent::ToolResponse(response) = content else {
                    continue;
                };
                let Ok(result) = &response.tool_result else {
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
        Some("file_lines" | "file_content" | "file_written") => text(&data["path"])
            .map(|path| vec![(path, text(&data["revision"]))])
            .unwrap_or_default(),
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
    use goose_provider_types::conversation::message::Message;
    use rmcp::model::CallToolResult;
    use serde_json::json;

    fn result(id: &str, value: Value) -> Message {
        Message::user()
            .with_generated_id()
            .with_tool_response(id, Ok(CallToolResult::structured(value)))
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
        ];
        let observed = Observed::collect(Some(&Conversation::new_unvalidated(messages)));
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
        let observed = Observed::collect(Some(&Conversation::new_unvalidated(messages)));
        for path in ["failed.rs", "other.rs", "gone.rs"] {
            assert_eq!(observed.revision(path), None, "{path}");
        }
        assert_eq!(Observed::collect(None).revision("failed.rs"), None);
    }
}
