//! Native code services stay generic; context composition belongs to the package.
use super::*;
use crate::agent::tools::decode;
use sailry_protocol::code_index::{Outline, SearchPage, Symbol};

pub(super) fn handles(operation: Operation) -> bool {
    matches!(operation, Operation::SearchCode | Operation::OutlineCode)
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::SearchCode => {
            "Find where symbols are defined in the captured worktree; lines are one-based"
        }
        Operation::OutlineCode => "List one file's declarations with one-based line ranges",
        _ => unreachable!("code operation expected"),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    match operation {
        Operation::SearchCode => {
            json!({"type":"object","additionalProperties":false,"properties":{"query":{"type":"string","maxLength":4096,"description":"Identifier or words to match against symbol names, paths and signatures"},"name":{"type":"string","maxLength":512,"description":"Case-insensitive symbol-name substring"},"path":{"type":"string","description":"Worktree-relative file or directory"},"language":{"type":"string","maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":100}}})
        }
        Operation::OutlineCode => {
            json!({"type":"object","additionalProperties":false,"properties":{"path":{"type":"string","minLength":1}},"required":["path"]})
        }
        _ => unreachable!("code operation expected"),
    }
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    let worktree = context
        .worktree
        .ok_or_else(|| Fault::invalid("code operation requires a worktree"))?;
    match operation {
        Operation::SearchCode => Ok(Command::SearchCode {
            worktree,
            query: decode(arguments)?,
        }),
        Operation::OutlineCode => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Outline {
                path: String,
            }
            let input: Outline = decode(arguments)?;
            Ok(Command::OutlineCode {
                worktree,
                path: input.path,
            })
        }
        _ => Err(Fault::invalid("code operation expected")),
    }
}

/// Shapes a code result for the model. Plugin flows and clients keep the full
/// protocol result with byte ranges and per-symbol revisions.
pub(super) fn output(operation: Operation, data: Value) -> Result<Value, Fault> {
    let invalid = |error: serde_json::Error| Fault::new(ErrorCode::Internal, error.to_string());
    Ok(match operation {
        Operation::SearchCode => {
            let page: SearchPage = serde_json::from_value(data).map_err(invalid)?;
            json!({
                "symbols": page.symbols.iter().map(|item| symbol(item, true)).collect::<Vec<_>>(),
                "index_revision": page.index_revision,
                "partial": page.partial,
                "truncated": page.truncated,
                "skipped": page.skipped,
                "issues": page.issues,
            })
        }
        Operation::OutlineCode => {
            let outline: Outline = serde_json::from_value(data).map_err(invalid)?;
            json!({
                "path": outline.path,
                "language": outline.language,
                "file_revision": outline.file_revision,
                "index_revision": outline.index_revision,
                "status": outline.status,
                "symbols": outline.symbols.iter().map(|item| symbol(item, false)).collect::<Vec<_>>(),
                "capabilities": outline.capabilities,
                "partial": outline.partial,
                "truncated": outline.truncated,
            })
        }
        _ => data,
    })
}

/// Converts zero-based ranges with exclusive byte ends to one-based inclusive lines.
fn symbol(symbol: &Symbol, located: bool) -> Value {
    let range = &symbol.range;
    let start = range.start_line + 1;
    let end = (range.end_line + u32::from(range.end_column > 0)).max(start);
    let mut value = json!({
        "name": symbol.name,
        "kind": symbol.kind,
        "depth": symbol.depth,
        "start_line": start,
        "end_line": end,
    });
    if located {
        value["path"] = symbol.path.clone().into();
        value["language"] = symbol.language.clone().into();
    }
    if let Some(signature) = &symbol.signature {
        value["signature"] = signature.clone().into();
    }
    value
}

pub(super) struct Cancellation {
    queries: Arc<crate::code_index::Queries>,
    caller: sailry_protocol::NodeId,
    request: sailry_protocol::RequestId,
    active: bool,
}

impl Cancellation {
    pub(super) fn new(
        ingress: &Ingress,
        caller: sailry_protocol::NodeId,
        request: &Request,
    ) -> Option<Self> {
        matches!(
            request.command,
            Command::SearchCode { .. } | Command::OutlineCode { .. }
        )
        .then(|| Self {
            queries: ingress.code_queries.clone(),
            caller,
            request: request.id,
            active: true,
        })
    }
    pub(super) fn finish(&mut self) {
        self.active = false;
    }
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        if self.active {
            self.queries.cancel(self.caller, self.request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::code_index::{Issue, Range, Status};

    fn page() -> SearchPage {
        SearchPage {
            symbols: vec![Symbol {
                path: "crates/node-runtime/src/code_index/index.rs".into(),
                language: "rust".into(),
                name: "search".into(),
                kind: "method".into(),
                range: Range {
                    start_byte: 9000,
                    end_byte: 12000,
                    start_line: 311,
                    start_column: 4,
                    end_line: 365,
                    end_column: 5,
                },
                signature: Some(
                    "pub(super) async fn search(&self, query: &Search, stop: &CancellationToken) -> Result<SearchPage, Fault>".into(),
                ),
                file_revision: "a".repeat(64),
                depth: 1,
            }],
            index_revision: 42,
            partial: false,
            truncated: false,
            visited: 900,
            skipped: 1,
            issues: vec![Issue {
                path: "vendor/large.rs".into(),
                status: Status::TooLarge,
            }],
        }
    }

    #[test]
    fn keeps_search_hits_compact() {
        let output = output(Operation::SearchCode, serde_json::to_value(page()).unwrap()).unwrap();
        assert!(
            serde_json::to_vec(&output).unwrap().len() < 1536,
            "{output}"
        );
        let symbol = &output["symbols"][0];
        assert_eq!(symbol["start_line"], 312);
        assert_eq!(symbol["end_line"], 366);
        assert_eq!(
            symbol["path"],
            "crates/node-runtime/src/code_index/index.rs"
        );
        assert!(symbol.get("range").is_none() && symbol.get("file_revision").is_none());
        assert_eq!(output["index_revision"], 42);
        assert_eq!(output["skipped"], 1);
        assert_eq!(output["issues"][0]["status"], "too_large");
        assert!(output.get("visited").is_none());
    }

    #[test]
    fn ends_before_a_zero_column() {
        let mut page = page();
        page.symbols[0].range.end_column = 0;
        page.symbols[0].signature = None;
        let output = output(Operation::SearchCode, serde_json::to_value(page).unwrap()).unwrap();
        assert_eq!(output["symbols"][0]["end_line"], 365);
        assert!(output["symbols"][0].get("signature").is_none());
    }
}
