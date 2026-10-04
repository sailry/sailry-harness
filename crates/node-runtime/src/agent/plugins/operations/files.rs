//! File and document capabilities use the invocation's captured worktree.
use super::*;
use crate::agent::tools::decode;
use sailry_protocol::{DirectoryCursor, FileSearch, office};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    path: String,
    after: Option<DirectoryCursor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    path: String,
    text: String,
    expected_revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    regex: bool,
    case_sensitive: bool,
    globs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::ListDirectory => "List a directory in the captured worktree",
        Operation::ReadFile => "Read bounded UTF-8 text in the captured worktree",
        Operation::WriteFile => "Write UTF-8 text using the expected file revision",
        Operation::SearchFiles => "Search text in the captured worktree",
        Operation::OfficeRuntime => "Read the execution Node's document authoring environment",
        Operation::ReadOffice => "Read document sections in the captured worktree",
        Operation::ExportPdf => "Export a document to PDF in the captured worktree",
        _ => unreachable!("not a file operation"),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    let path = json!({"type":"string"});
    let (properties, required) = match operation {
        Operation::ListDirectory => (
            json!({"path":path,"after":{"type":["object","null"],"properties":{"revision":{"type":"string"},"directory":{"type":"boolean"},"name":{"type":"string"}},"required":["revision","directory","name"],"additionalProperties":false}}),
            vec!["path", "after"],
        ),
        Operation::ReadFile => (json!({"path":path}), vec!["path"]),
        Operation::WriteFile => (
            json!({"path":path,"text":{"type":"string"},"expected_revision":{"type":["string","null"]}}),
            vec!["path", "text", "expected_revision"],
        ),
        Operation::SearchFiles => (
            json!({"query":{"type":"string"},"regex":{"type":"boolean"},"case_sensitive":{"type":"boolean"},"globs":{"type":"array","items":{"type":"string"}}}),
            vec!["query", "regex", "case_sensitive", "globs"],
        ),
        Operation::OfficeRuntime => (json!({}), vec![]),
        Operation::ReadOffice => {
            return serde_json::to_value(schemars::schema_for!(office::Read))
                .expect("Office schema is JSON");
        }
        Operation::ExportPdf => {
            return serde_json::to_value(schemars::schema_for!(office::Export))
                .expect("Office schema is JSON");
        }
        _ => unreachable!("not a file operation"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    context: &plugin::Context,
) -> Result<Command, Fault> {
    let worktree = context.worktree.ok_or_else(|| {
        Fault::new(
            ErrorCode::InvalidRequest,
            "file operation requires a worktree",
        )
    })?;
    Ok(match operation {
        Operation::ListDirectory => {
            let input: Directory = decode(arguments)?;
            Command::ListDirectory {
                worktree,
                path: input.path,
                after: input.after,
            }
        }
        Operation::ReadFile => {
            let input: File = decode(arguments)?;
            Command::ReadFile {
                worktree,
                path: input.path,
            }
        }
        Operation::WriteFile => {
            let input: Write = decode(arguments)?;
            Command::WriteFile {
                worktree,
                path: input.path,
                text: input.text,
                expected_revision: input.expected_revision,
            }
        }
        Operation::SearchFiles => {
            let input: Search = decode(arguments)?;
            Command::SearchFiles {
                worktree,
                options: FileSearch {
                    query: input.query,
                    regex: input.regex,
                    case_sensitive: input.case_sensitive,
                    globs: input.globs,
                },
            }
        }
        Operation::OfficeRuntime => {
            let _: Empty = decode(arguments)?;
            Command::OfficeRuntime { worktree }
        }
        Operation::ReadOffice => Command::ReadOffice {
            worktree,
            options: decode(arguments)?,
        },
        Operation::ExportPdf => Command::ExportPdf {
            worktree,
            options: decode(arguments)?,
        },
        _ => unreachable!("not a file operation"),
    })
}
