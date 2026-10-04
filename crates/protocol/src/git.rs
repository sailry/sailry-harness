use serde::{Deserialize, Serialize};

pub const MAX_GIT_ENTRIES: usize = 500;
pub const MAX_DIFF_BYTES: usize = 48 * 1024;
pub const MAX_GIT_COMMITS: usize = 100;
pub const MAX_GIT_WORKTREES: usize = 100;

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTag {
    pub name: String,
    pub commit: String,
}
