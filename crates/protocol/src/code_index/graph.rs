//! Name-resolved references between indexed definitions. Names are matched
//! without type information, so every resolution records how it was made.
use super::Issue;
use serde::{Deserialize, Serialize};

pub const DEFAULT_GRAPH_ITEMS: usize = 50;
pub const MAX_GRAPH_ITEMS: usize = 200;
pub const MAX_GRAPH_OFFSET: usize = 10_000;
pub const MAX_GRAPH_DEPTH: u8 = 3;
pub const MAX_GRAPH_PATHS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// References to the selected definitions.
    Callers,
    /// References inside the selected definitions.
    Callees,
    /// Callers, callees and impacted files of the selected definitions.
    Context,
    /// Definitions reached by reverse references, grouped by file.
    Impact,
    /// Files ranked by incoming references, optionally personalized by paths.
    Map,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub view: View,
    /// Exact definition name.
    #[serde(default)]
    pub name: Option<String>,
    /// Confined worktree-relative files or directories. They narrow selected
    /// definitions, seed impact with every definition they contain or focus the map.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Zero-based line inside the selected definition of exactly one path.
    #[serde(default)]
    pub line: Option<u32>,
    /// Reverse reference hops for impact.
    #[serde(default = "depth")]
    pub depth: u8,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "limit")]
    pub limit: usize,
}

impl Graph {
    pub fn new(view: View) -> Self {
        Self {
            view,
            name: None,
            paths: Vec::new(),
            line: None,
            depth: MAX_GRAPH_DEPTH,
            offset: 0,
            limit: DEFAULT_GRAPH_ITEMS,
        }
    }
}

fn depth() -> u8 {
    MAX_GRAPH_DEPTH
}

fn limit() -> usize {
    DEFAULT_GRAPH_ITEMS
}

/// How a referenced name was matched to definitions, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Defined in the referencing file.
    Local,
    /// Defined in a file the referencing file imports.
    Imported,
    /// Defined in the referencing file's directory.
    Directory,
    /// Defined elsewhere in the worktree.
    Workspace,
}

/// Zero-based inclusive lines of one indexed definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// References from one place to one name that share a resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub path: String,
    pub name: String,
    pub kind: String,
    /// Zero-based position of the first reference; columns count UTF-8 bytes.
    pub line: u32,
    pub column: u32,
    pub count: u32,
    /// Innermost definition containing the reference.
    pub scope: Option<Definition>,
    /// Matched definitions at the strongest tier, at most three.
    pub targets: Vec<Definition>,
    /// Definitions sharing that tier; more than one is ambiguous.
    pub candidates: usize,
    /// Absent when no indexed definition has this name.
    pub confidence: Option<Confidence>,
    pub ambiguous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Affected {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub depth: u8,
    /// The weakest resolution along the strongest path.
    pub confidence: Confidence,
    /// Every path reaching this definition crosses an ambiguous reference.
    pub ambiguous: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impacted {
    pub path: String,
    pub depth: u8,
    pub confidence: Confidence,
    pub ambiguous: bool,
    /// Empty when only top-level code refers to the selection.
    pub symbols: Vec<Affected>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ranked {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    /// Weighted references from other files.
    pub references: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    /// Rank in thousandths of the highest-ranked file.
    pub score: u32,
    pub symbols: Vec<Ranked>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphPage {
    pub view: View,
    pub index_revision: u64,
    /// Definitions selected by name, path and line.
    pub targets: Vec<Definition>,
    pub callers: Vec<Reference>,
    pub callees: Vec<Reference>,
    pub impact: Vec<Impacted>,
    pub map: Vec<Entry>,
    /// Items before paging in the view's list; impacted files for context.
    pub total: usize,
    pub next_offset: Option<usize>,
    pub partial: bool,
    pub truncated: bool,
    pub visited: usize,
    pub skipped: usize,
    pub issues: Vec<Issue>,
}

impl GraphPage {
    pub fn new(view: View) -> Self {
        Self {
            view,
            index_revision: 0,
            targets: Vec::new(),
            callers: Vec::new(),
            callees: Vec::new(),
            impact: Vec::new(),
            map: Vec::new(),
            total: 0,
            next_offset: None,
            partial: false,
            truncated: false,
            visited: 0,
            skipped: 0,
            issues: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_to_bounded_queries() {
        let query: Graph = serde_json::from_value(json!({"view":"impact","name":"run"})).unwrap();
        assert_eq!(query.depth, MAX_GRAPH_DEPTH);
        assert_eq!(query.limit, DEFAULT_GRAPH_ITEMS);
        assert_eq!(query.offset, 0);
        assert!(query.paths.is_empty() && query.line.is_none());
        assert!(serde_json::from_value::<Graph>(json!({"view":"map","unknown":1})).is_err());
        assert!(serde_json::from_value::<Graph>(json!({"view":"references"})).is_err());
    }

    #[test]
    fn orders_confidence_from_strongest() {
        assert!(Confidence::Local < Confidence::Imported);
        assert!(Confidence::Imported < Confidence::Directory);
        assert!(Confidence::Directory < Confidence::Workspace);
        assert_eq!(
            serde_json::to_value(Confidence::Workspace).unwrap(),
            json!("workspace")
        );
        let page = GraphPage::new(View::Context);
        let value = serde_json::to_value(&page).unwrap();
        assert_eq!(value["view"], "context");
        assert_eq!(serde_json::from_value::<GraphPage>(value).unwrap(), page);
    }
}
