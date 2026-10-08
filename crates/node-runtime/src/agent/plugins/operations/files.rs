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
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    200
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
struct Edit {
    path: String,
    old_text: String,
    new_text: String,
    expected_revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Find {
    globs: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    query: String,
    regex: bool,
    case_sensitive: bool,
    globs: Vec<String>,
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::ListDirectory => "List a directory in the captured worktree",
        Operation::ReadFile => {
            "Read numbered lines with a full file revision; offset starts at zero and defaults to 0, limit defaults to 200"
        }
        Operation::WriteFile => "Write UTF-8 text using the expected file revision",
        Operation::EditFile => {
            "Replace old_text exactly once using the full current file revision; preserve all surrounding bytes"
        }
        Operation::FindFiles => "Find regular file paths by glob without reading their contents",
        Operation::SearchFiles => "Search text in the captured worktree",
        Operation::ReadOffice => "Read document sections in the captured worktree",
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
        Operation::ReadFile => (
            json!({"path":path,"offset":{"type":"integer","minimum":0,"default":0},"limit":{"type":"integer","minimum":1,"maximum":sailry_protocol::MAX_FILE_LINES,"default":200}}),
            vec!["path"],
        ),
        Operation::WriteFile => (
            json!({"path":path,"text":{"type":"string"},"expected_revision":{"type":["string","null"]}}),
            vec!["path", "text", "expected_revision"],
        ),
        Operation::EditFile => (
            json!({"path":path,"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"expected_revision":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}),
            vec!["path", "old_text", "new_text", "expected_revision"],
        ),
        Operation::FindFiles => (
            json!({"globs":{"type":"array","items":{"type":"string","minLength":1,"maxLength":4096},"minItems":1,"maxItems":FileSearch::MAX_GLOBS}}),
            vec!["globs"],
        ),
        Operation::SearchFiles => (
            json!({"query":{"type":"string"},"regex":{"type":"boolean"},"case_sensitive":{"type":"boolean"},"globs":{"type":"array","items":{"type":"string"}}}),
            vec!["query", "regex", "case_sensitive", "globs"],
        ),
        Operation::ReadOffice => {
            return serde_json::to_value(schemars::schema_for!(office::Read))
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
    let worktree = context
        .worktree
        .ok_or_else(|| Fault::invalid("file operation requires a worktree"))?;
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
            Command::ReadFileLines {
                worktree,
                path: input.path,
                offset: input.offset,
                limit: input.limit,
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
        Operation::EditFile => {
            let input: Edit = decode(arguments)?;
            Command::EditFile {
                worktree,
                path: input.path,
                old_text: input.old_text,
                new_text: input.new_text,
                expected_revision: input.expected_revision,
            }
        }
        Operation::FindFiles => {
            let input: Find = decode(arguments)?;
            Command::FindFiles {
                worktree,
                globs: input.globs,
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
        Operation::ReadOffice => Command::ReadOffice {
            worktree,
            options: decode(arguments)?,
        },
        _ => unreachable!("not a file operation"),
    })
}
