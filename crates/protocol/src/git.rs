use serde::{Deserialize, Serialize};

pub const MAX_GIT_ENTRIES: usize = 500;
pub const MAX_DIFF_BYTES: usize = 48 * 1024;
pub const MAX_GIT_COMMITS: usize = 100;
pub const MAX_GIT_WORKTREES: usize = 100;
pub const MAX_GIT_LINES: usize = 500;
pub const MAX_GIT_FILE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GitChangeTarget {
    Worktree { scope: GitDiffScope },
    Commit { revision: String },
    Range { base: String, head: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum GitChangeState {
    Worktree {
        scope: GitDiffScope,
        head: Option<String>,
        index_revision: String,
    },
    Commit {
        revision: String,
        parent: Option<String>,
    },
    Range {
        base: String,
        head: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitChangeSnapshot {
    pub target: GitChangeState,
    pub metadata_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitChangesCursor {
    pub snapshot: GitChangeSnapshot,
    /// Null starts again from the frozen snapshot without resolving references.
    pub after_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChangeEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: GitChangeKind,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub metadata_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChanges {
    pub snapshot: GitChangeSnapshot,
    pub entries: Vec<GitChangeEntry>,
    pub next: Option<GitChangesCursor>,
    /// True only after every supported path was enumerated without omissions.
    pub exhausted: bool,
    /// A lower bound when a scan resource limit prevents counting the rest.
    pub omitted_paths: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeLineKind {
    Added,
    Deleted,
    Context,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChangeLine {
    pub kind: GitChangeLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// Complete source line, excluding terminal CR/LF and preserving other whitespace.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub snapshot: GitChangeSnapshot,
    pub path: String,
    pub old_path: Option<String>,
    pub metadata_revision: String,
    /// Strong identity of saved diff bytes; skipped unread resources use metadata identity.
    pub revision: String,
    pub offset: usize,
    pub next: Option<usize>,
    pub lines: Vec<GitChangeLine>,
    pub binary: bool,
    pub skip_reason: Option<String>,
    pub first_line: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFile {
    pub commit: String,
    pub path: String,
    /// Zero-based line offset; rendered text uses one-based line numbers.
    pub offset: usize,
    pub next: Option<usize>,
    pub total_lines: usize,
    pub text: String,
    pub binary: bool,
    pub truncated: bool,
    /// A line whose text exceeded the page byte bound, if any.
    pub truncated_line: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBlame {
    pub commit: String,
    pub path: String,
    pub offset: usize,
    pub next: Option<usize>,
    pub total_lines: usize,
    pub entries: Vec<GitBlameLine>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBlameLine {
    pub line: usize,
    pub original_line: usize,
    pub commit: String,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub text: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Native Git discovery, independent of the Node's persisted execution contexts.
pub struct GitWorktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    /// The primary Git checkout, not necessarily the project's default context.
    pub main: bool,
    pub locked: bool,
    /// The directory and native registration were valid when inspected.
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktrees {
    pub kind: RepositoryKind,
    pub entries: Vec<GitWorktree>,
    pub truncated: bool,
    pub omitted_paths: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBranch {
    pub name: String,
    pub commit: String,
    pub remote: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBranches {
    pub sync: GitSync,
    pub kind: RepositoryKind,
    pub current: Option<String>,
    pub entries: Vec<GitBranch>,
    pub truncated: bool,
    pub omitted_names: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitMergeKind {
    UpToDate,
    FastForward,
    MergeCommit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitMerge {
    pub kind: GitMergeKind,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLogEntry {
    pub id: String,
    pub message: String,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub references: Vec<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLogCursor {
    /// Immutable traversal root; continuation does not follow a moving HEAD.
    pub head: String,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLog {
    pub kind: RepositoryKind,
    pub head: Option<String>,
    pub offset: usize,
    pub next: Option<GitLogCursor>,
    pub entries: Vec<GitLogEntry>,
    pub truncated: bool,
    pub references_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    pub entry: GitLogEntry,
    /// Bounded per-file diffs against the first parent, or the empty tree for a root commit.
    pub files: Vec<GitDiff>,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryKind {
    Directory,
    Unborn,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitEntry {
    pub diff: GitLineStats,
    pub staged_diff: GitLineStats,
    pub unstaged_diff: GitLineStats,
    pub path: String,
    pub staged: Option<GitChangeKind>,
    pub unstaged: Option<GitChangeKind>,
    pub untracked: bool,
    pub conflicted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatus {
    pub kind: RepositoryKind,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub index_revision: Option<String>,
    pub entries: Vec<GitEntry>,
    pub truncated: bool,
    pub omitted_paths: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffScope {
    All,
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiff {
    pub path: String,
    pub scope: GitDiffScope,
    pub text: String,
    pub additions: usize,
    pub deletions: usize,
    pub binary: bool,
    /// Counts and text are partial when a rendering bound was reached.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitIndexChange {
    Stage,
    Unstage,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitIndex {
    pub revision: String,
}

/// Closed set of Git operations, executed on the owning Node with its Git configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitAction {
    /// Initialize the registered directory without replacing existing Git metadata.
    Initialize,
    /// Remove only the Git registration for one missing linked worktree.
    PruneWorktree {
        path: String,
    },
    /// Select the branch name for a repository without its first commit.
    StartBranch {
        name: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    Fetch {
        remote: Option<String>,
        prune: bool,
        all: bool,
    },
    Pull {
        rebase: bool,
        remote: Option<String>,
        branch: Option<String>,
    },
    Sync {
        rebase: bool,
    },
    RemoveRemote {
        name: String,
    },
    DeleteRemoteBranch {
        branch: String,
        commit: String,
    },
    CreateTag {
        name: String,
        commit: String,
        message: Option<String>,
    },
    DeleteTag {
        name: String,
        commit: String,
        remote: Option<String>,
    },
    PushTags {
        remote: String,
    },
    DropAllStashes {
        commits: Vec<String>,
    },
    SwitchTracking {
        remote: String,
        local: String,
        stash: bool,
    },
    SwitchStashing {
        branch: String,
        commit: String,
    },
    Push {
        remote: Option<String>,
        publish: bool,
        force: bool,
    },
    Merge {
        commit: String,
    },
    Rebase {
        commit: String,
    },
    CherryPick {
        commit: String,
    },
    Track {
        branch: String,
    },
    SwitchRemote {
        branch: String,
    },
    UndoCommit,
    Stash {
        mode: GitStashMode,
    },
    ApplyStash {
        commit: String,
        pop: bool,
    },
    DropStash {
        commit: String,
    },
    DiscardAll {
        paths: Vec<String>,
    },
    /// Add literal worktree-relative paths to the root .gitignore.
    Ignore {
        paths: Vec<String>,
        directories: bool,
    },
    Discard {
        paths: Vec<String>,
    },
    Trash {
        paths: Vec<String>,
    },
    Continue,
    Abort,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSync {
    pub tags: Vec<GitTag>,
    pub remote_urls: std::collections::BTreeMap<String, String>,
    pub tracking: std::collections::BTreeMap<String, String>,
    pub stashes: Vec<GitStash>,
    pub head_summary: Option<String>,
    pub head_message: Option<String>,
    pub remotes: Vec<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub operation: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommitOptions {
    pub signoff: bool,
    pub skip_hooks: bool,
    pub tracked: bool,
    pub all: bool,
    pub after: GitAfterCommit,
    pub push_remote: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLineStats {
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStashMode {
    All,
    Tracked,
    Staged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStash {
    pub commit: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitAfterCommit {
    #[default]
    None,
    Push,
    Sync,
}

#[cfg(test)]
mod changes_tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTag {
    pub name: String,
    pub commit: String,
}
