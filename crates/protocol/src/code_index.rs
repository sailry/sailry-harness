//! Rebuildable worktree code structure, independent of language servers.
use serde::{Deserialize, Serialize};

pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const DEFAULT_SEARCH_SYMBOLS: usize = 20;
pub const MAX_SEARCH_SYMBOLS: usize = 100;
pub const MAX_OUTLINE_SYMBOLS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    /// Literal full-text terms, not a database query expression.
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub name: Option<String>,
    /// A confined path prefix relative to the selected worktree.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default = "search_limit")]
    pub limit: usize,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            query: String::new(),
            name: None,
            path: None,
            language: None,
            limit: DEFAULT_SEARCH_SYMBOLS,
        }
    }
}

fn search_limit() -> usize {
    DEFAULT_SEARCH_SYMBOLS
}

/// Zero-based source positions. Columns count UTF-8 bytes; byte ends are exclusive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub range: Range,
    pub signature: Option<String>,
    /// Full-content BLAKE3 revision used to produce the source range.
    pub file_revision: String,
    pub depth: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Parsed,
    Unsupported,
    TooLarge,
    Invalid,
    Failed,
}

/// Capabilities observed in this file's extraction, not promises for its grammar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub structure: bool,
    pub symbols: bool,
    pub data: bool,
    pub embedded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub path: String,
    pub status: Status,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    pub symbols: Vec<Symbol>,
    pub index_revision: u64,
    pub partial: bool,
    pub truncated: bool,
    pub visited: usize,
    pub skipped: usize,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    pub path: String,
    pub language: Option<String>,
    pub file_revision: Option<String>,
    pub index_revision: u64,
    pub status: Status,
    /// Source-ordered declarations; depth expresses their structural nesting.
    pub symbols: Vec<Symbol>,
    pub capabilities: Capabilities,
    pub partial: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_bounded_search() {
        let search: Search = serde_json::from_value(json!({})).unwrap();
        assert_eq!(search, Search::default());
        assert_eq!(search.limit, 20);
    }

    #[test]
    fn preserves_utf8_coordinates() {
        let range = Range {
            start_byte: 3,
            end_byte: 9,
            start_line: 0,
            start_column: 3,
            end_line: 0,
            end_column: 9,
        };
        let value = serde_json::to_value(&range).unwrap();
        assert_eq!(value["start_column"], 3);
        assert_eq!(value["end_byte"], 9);
        assert_eq!(serde_json::from_value::<Range>(value).unwrap(), range);
    }

    #[test]
    fn queries_use_shared_read_commands() {
        let worktree = crate::WorktreeId::new();
        for command in [
            crate::Command::SearchCode {
                worktree,
                query: Search::default(),
            },
            crate::Command::OutlineCode {
                worktree,
                path: "src/中文.rs".into(),
            },
            crate::Command::ReadFileLines {
                worktree,
                path: "src/中文.rs".into(),
                offset: 1,
                limit: 20,
                expected_revision: Some("a".repeat(64)),
            },
            crate::Command::CancelCodeIndexQuery {
                request: crate::RequestId::new(),
            },
            crate::Command::ReadCodeIndexSettings,
        ] {
            assert!(!command.durable());
            assert_eq!(
                serde_json::from_value::<crate::Command>(serde_json::to_value(&command).unwrap())
                    .unwrap(),
                command
            );
        }
        assert_eq!(crate::VERSION, 1);
    }

    #[test]
    fn declares_current_read_capabilities() {
        use crate::{plugin::Action, tool::Operation};
        for (name, operation, action) in [
            ("code.search", Operation::SearchCode, Action::ReadCodeIndex),
            (
                "code.outline",
                Operation::OutlineCode,
                Action::ReadCodeIndex,
            ),
            (
                "files.read_lines",
                Operation::ReadFileLines,
                Action::ReadFiles,
            ),
        ] {
            assert_eq!(serde_json::to_value(operation).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<Operation>(json!(name)).unwrap(),
                operation
            );
            assert_eq!(operation.action(), Some(action));
            assert!(operation.current());
            assert!(operation.read_only());
        }
        let service = crate::plugin::Service::CodeIndex;
        assert!(service.current());
        assert_eq!(serde_json::to_value(service).unwrap(), json!("code_index"));
    }
}
