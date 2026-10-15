//! File and document capabilities use the invocation's captured worktree.
use super::*;
use crate::agent::tools::decode;
use sailry_protocol::{DirectoryCursor, FileEdit, FileSearch, office};
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
    old_text: Option<String>,
    new_text: Option<String>,
    #[serde(default)]
    replace_all: bool,
    edits: Option<Vec<FileEdit>>,
    expected_revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    path: String,
    expected_revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    path: String,
    destination: String,
    expected_revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Find {
    globs: Vec<String>,
    #[serde(default)]
    path: String,
    #[serde(default = "find_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn find_limit() -> usize {
    sailry_protocol::MAX_FIND_MATCHES
}

pub(super) fn failure(operation: Operation, error: Fault) -> Value {
    use sailry_protocol::tool::{Diagnostic, Expected, ResultDisplay};
    let expected = (error.code == ErrorCode::NotFound)
        .then_some(match operation {
            Operation::ListDirectory => Some(Expected::DirectoryNotFound),
            Operation::ReadFile => Some(Expected::FileNotFound),
            _ => None,
        })
        .flatten();
    let mut result = json!({"error":error,"isError":true});
    if let Some(expected) = expected {
        result["sailry_result"] = json!(ResultDisplay {
            version: 1,
            diagnostics: vec![Diagnostic {
                text: error.message,
                error: false,
            }],
            status: None,
            expected: Some(expected),
        });
    }
    result
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::ListDirectory => "List a directory in the captured worktree",
        Operation::ReadFile => {
            "Read numbered lines with a full file revision; offset starts at zero and defaults to 0, limit defaults to 200"
        }
        Operation::WriteFile => "Write UTF-8 text using the expected file revision",
        Operation::EditFile => {
            "Apply one replacement or a batch atomically using the current file revision. Match each edit against the original file; require a unique match unless replace_all is true and reject overlaps"
        }
        Operation::DeleteFile => "Move a regular file to Trash using its current revision",
        Operation::MoveFile => {
            "Move a regular file within the worktree using its current revision; never replace an existing destination"
        }
        Operation::CreateDirectory => {
            "Create a directory in an existing parent; an existing directory is unchanged"
        }
        Operation::FindFiles => {
            "Find regular file paths by glob within path, respecting .gitignore and never following symlinks. Newest files come first. Use next_offset for another page; partial scans have no continuation"
        }
        Operation::SearchFiles => {
            "Search UTF-8 files within path using embedded ripgrep matching, respecting .gitignore and never following symlinks. Choose content rows, matching paths, or matching line counts. type uses ripgrep language names; context adds up to five neighboring lines; multiline permits newline-spanning matches. Use next_offset for complete-scan pagination; narrow path if partial"
        }
        Operation::ReadOffice => {
            "Read document sections from a worktree path or a current-turn attachment ID"
        }
        Operation::ReadNotebook => {
            "Read numbered notebook cells without executing a kernel. Use next_offset to continue; truncated cells omit oversized metadata or outputs"
        }
        Operation::EditNotebook => {
            "Atomically replace cell sources, insert cells or delete cells using the notebook revision. Indices refer to the original notebook; untouched metadata, IDs and outputs are retained. No kernel is executed"
        }
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
        Operation::EditFile => return edit_schema(),
        Operation::DeleteFile => (
            json!({"path":path,"expected_revision":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}),
            vec!["path", "expected_revision"],
        ),
        Operation::MoveFile => (
            json!({"path":path,"destination":{"type":"string"},"expected_revision":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}),
            vec!["path", "destination", "expected_revision"],
        ),
        Operation::CreateDirectory => (json!({"path":path}), vec!["path"]),
        Operation::FindFiles => (
            json!({"globs":{"type":"array","items":{"type":"string","minLength":1,"maxLength":4096},"minItems":1,"maxItems":FileSearch::MAX_GLOBS},"path":{"type":"string","default":""},"limit":{"type":"integer","minimum":1,"maximum":500,"default":500},"offset":{"type":"integer","minimum":0,"default":0}}),
            vec!["globs"],
        ),
        Operation::SearchFiles => (
            json!({"query":{"type":"string","minLength":1},"regex":{"type":"boolean","default":false},"case_sensitive":{"type":"boolean","default":false},"globs":{"type":"array","items":{"type":"string"},"default":[]},"path":{"type":"string","default":""},"mode":{"type":"string","enum":["content","files","count"],"default":"content"},"context":{"type":"integer","minimum":0,"maximum":5,"default":0},"type":{"type":["string","null"]},"limit":{"type":"integer","minimum":1,"maximum":200,"default":200},"offset":{"type":"integer","minimum":0,"default":0},"multiline":{"type":"boolean","default":false}}),
            vec!["query"],
        ),
        Operation::ReadNotebook => {
            return serde_json::to_value(schemars::schema_for!(sailry_protocol::notebook::Read))
                .expect("Notebook read schema is JSON");
        }
        Operation::EditNotebook => {
            return serde_json::to_value(schemars::schema_for!(sailry_protocol::notebook::Edit))
                .expect("Notebook edit schema is JSON");
        }
        Operation::ReadOffice => {
            return serde_json::to_value(schemars::schema_for!(office::Read))
                .expect("Office schema is JSON");
        }
        _ => unreachable!("not a file operation"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn edit_schema() -> Value {
    let old = json!({"type":"string","minLength":1});
    let new = json!({"type":"string"});
    let all = json!({"type":"boolean","default":false});
    json!({"type":"object", "properties":{
    "path":{"type":"string"}, "old_text":old, "new_text":new, "replace_all":all,
    "edits":{"type":"array","minItems":1,"maxItems":sailry_protocol::MAX_FILE_EDITS,
        "items":{"type":"object","properties":{"old_text":old,"new_text":new,"replace_all":all},
            "required":["old_text","new_text"],"additionalProperties":false}},
    "expected_revision":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}},
    "required":["path","expected_revision"], "additionalProperties":false,
    "oneOf":[
        {"required":["old_text","new_text"],"not":{"required":["edits"]}},
        {"required":["edits"],"not":{"anyOf":[{"required":["old_text"]},{"required":["new_text"]}]},
            "properties":{"replace_all":{"const":false}}}
    ]})
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
            let text =
                crate::files::notebook::prepare_creation(&input.path, &input.text)?.into_owned();
            Command::WriteFile {
                worktree,
                path: input.path,
                text,
                expected_revision: input.expected_revision,
            }
        }
        Operation::EditFile => {
            let input: Edit = decode(arguments)?;
            crate::files::edit::changes(
                input.old_text.as_deref(),
                input.new_text.as_deref(),
                input.replace_all,
                input.edits.as_deref(),
            )?;
            Command::EditFile {
                worktree,
                path: input.path,
                old_text: input.old_text,
                new_text: input.new_text,
                replace_all: input.replace_all,
                edits: input.edits,
                expected_revision: input.expected_revision,
            }
        }
        Operation::DeleteFile => {
            let input: Delete = decode(arguments)?;
            Command::DeleteFile {
                worktree,
                path: input.path,
                expected_revision: input.expected_revision,
            }
        }
        Operation::MoveFile => {
            let input: Move = decode(arguments)?;
            Command::MoveFile {
                worktree,
                from: input.path,
                to: input.destination,
                expected_revision: input.expected_revision,
            }
        }
        Operation::CreateDirectory => {
            let input: Create = decode(arguments)?;
            Command::CreateDirectory {
                worktree,
                path: input.path,
            }
        }
        Operation::FindFiles => {
            let input: Find = decode(arguments)?;
            Command::FindFiles {
                worktree,
                globs: input.globs,
                path: input.path,
                limit: input.limit,
                offset: input.offset,
            }
        }
        Operation::SearchFiles => Command::SearchFiles {
            worktree,
            options: decode::<FileSearch>(arguments)?,
        },
        Operation::ReadNotebook => Command::ReadNotebook {
            worktree,
            options: decode(arguments)?,
        },
        Operation::EditNotebook => Command::EditNotebook {
            worktree,
            edit: decode(arguments)?,
        },
        Operation::ReadOffice => Command::ReadOffice {
            worktree,
            options: decode(arguments)?,
        },
        _ => unreachable!("not a file operation"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_only_expected_read_failures() {
        use sailry_protocol::tool::{Expected, ResultDisplay};
        for (operation, expected) in [
            (Operation::ListDirectory, Some(Expected::DirectoryNotFound)),
            (Operation::ReadFile, Some(Expected::FileNotFound)),
            (Operation::ReadOffice, None),
            (Operation::WriteFile, None),
        ] {
            let result = failure(operation, Fault::new(ErrorCode::NotFound, "Raw diagnostic"));
            assert_eq!(result["error"]["message"], "Raw diagnostic");
            assert_eq!(result["isError"], true);
            assert_eq!(
                ResultDisplay::from_value(&result).and_then(|display| display.expected),
                expected
            );
            let denied = failure(operation, Fault::new(ErrorCode::PermissionDenied, "Denied"));
            assert!(ResultDisplay::from_value(&denied).is_none());
        }
    }
}
