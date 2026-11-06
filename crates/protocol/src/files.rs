use serde::{Deserialize, Serialize};

pub const MAX_FILE_BYTES: usize = 128 * 1024;
pub const MAX_EDIT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FILE_LINES: usize = 2000;
pub const MAX_FIND_MATCHES: usize = 500;
pub const MAX_DIRECTORY_ENTRIES: usize = 500;
pub const MAX_SEARCH_MATCHES: usize = 200;
pub const FILE_TRANSFER_CHUNK_BYTES: usize = 64 * 1024;
/// Upload bytes are verified and staged. This is not publication or durable admission.
pub const FILE_UPLOAD_READY: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileUploadSpec {
    pub worktree: crate::WorktreeId,
    pub path: String,
    pub size: u64,
    /// Full-content BLAKE3 hash of the selected source.
    pub revision: String,
    /// None creates a new file; replacement requires the current target revision.
    pub expected_revision: Option<String>,
}

/// Single-use staging stream. FinishFileUpload publishes through durable admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileUpload {
    pub stream: crate::StreamId,
    pub spec: FileUploadSpec,
}

/// A single-use, caller-owned download stream. Bytes are not command/event payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDownload {
    pub stream: crate::StreamId,
    pub worktree: crate::WorktreeId,
    pub path: String,
    pub size: u64,
    /// Full-content BLAKE3 hash. Receivers must verify before publishing a download.
    pub revision: String,
    /// Opaque source identity and metadata revision for checked move-to-Trash.
    pub stamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileSearch {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub globs: Vec<String>,
    pub path: String,
    pub mode: SearchMode,
    pub context: usize,
    #[serde(rename = "type")]
    pub file_type: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub multiline: bool,
}

impl Default for FileSearch {
    fn default() -> Self {
        Self {
            query: String::new(),
            regex: false,
            case_sensitive: false,
            globs: Vec::new(),
            path: String::new(),
            mode: SearchMode::Content,
            context: 0,
            file_type: None,
            limit: MAX_SEARCH_MATCHES,
            offset: 0,
            multiline: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Content,
    Files,
    Count,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
    /// False marks a neighboring context line.
    pub matched: bool,
    /// The original line exceeds the bounded response preview.
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCount {
    pub path: String,
    pub count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDiagnostic {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SearchResults {
    pub mode: SearchMode,
    pub matches: Vec<SearchMatch>,
    pub paths: Vec<String>,
    pub counts: Vec<SearchCount>,
    pub scanned_files: usize,
    pub skipped: usize,
    pub truncated: bool,
    /// Incomplete scans have no reliable continuation offset.
    pub partial: bool,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub diagnostics: Vec<SearchDiagnostic>,
}

impl Serialize for SearchResults {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut value = serializer.serialize_struct("SearchResults", 9)?;
        value.serialize_field("mode", &self.mode)?;
        match self.mode {
            SearchMode::Content => value.serialize_field("matches", &self.matches)?,
            SearchMode::Files => value.serialize_field("paths", &self.paths)?,
            SearchMode::Count => value.serialize_field("counts", &self.counts)?,
        }
        value.serialize_field("scanned_files", &self.scanned_files)?;
        value.serialize_field("skipped", &self.skipped)?;
        value.serialize_field("truncated", &self.truncated)?;
        value.serialize_field("partial", &self.partial)?;
        value.serialize_field("offset", &self.offset)?;
        value.serialize_field("next_offset", &self.next_offset)?;
        value.serialize_field("diagnostics", &self.diagnostics)?;
        value.end()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
}

impl FileEntry {
    /// Directories first, then exact UTF-8 names; shared by pagination and views.
    pub fn sort_key(&self) -> (bool, &str) {
        (self.kind != EntryKind::Directory, &self.name)
    }
}

/// A read-only continuation, not a retained filesystem snapshot or authorization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCursor {
    pub revision: String,
    pub directory: bool,
    pub name: String,
}

impl DirectoryCursor {
    pub fn sort_key(&self) -> (bool, &str) {
        (!self.directory, &self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    /// Empty string denotes the registered worktree root.
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub truncated: bool,
    /// Names not representable in the UTF-8 protocol are never aliased to another file.
    pub unsupported_names: usize,
    /// Opaque metadata revision for ordinary directory-change detection.
    pub revision: String,
    pub next: Option<DirectoryCursor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContent {
    pub path: String,
    pub text: String,
    pub size: u64,
    pub truncated: bool,
    /// Full-content BLAKE3 revision. A truncated preview cannot authorize a write.
    pub revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLine {
    /// One-based position in the complete file.
    pub number: usize,
    /// Original text, including its line ending when present.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLines {
    pub path: String,
    pub lines: Vec<FileLine>,
    pub size: u64,
    /// Full-content BLAKE3 revision, including text outside this page.
    pub revision: String,
    /// Zero-based line offset requested by the caller.
    pub offset: usize,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileNames {
    pub paths: Vec<String>,
    pub visited: usize,
    pub skipped: usize,
    pub truncated: bool,
    pub partial: bool,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub diagnostics: Vec<SearchDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWritten {
    pub path: String,
    pub revision: String,
    pub size: u64,
    /// Bounded numbered regions around applied edits; absent for whole-file writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<FileExcerpt>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExcerpt {
    pub hunks: Vec<FileHunk>,
    /// Edited regions left out by the excerpt budget.
    #[serde(default)]
    pub omitted: usize,
}

/// Consecutive lines of the edited file, with a few lines of context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHunk {
    pub lines: Vec<FileLine>,
    /// Lines past the hunk budget or long line text were left out.
    #[serde(default)]
    pub shortened: bool,
}

/// Replacements match the original file, before any edit in the batch is applied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEdit {
    pub old_text: String,
    pub new_text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_all: bool,
}

/// Entries in one change set; every path and move destination appears once.
pub const MAX_CHANGES: usize = 16;
/// Original and resulting text one change set may hold for validation and checkpoints.
pub const MAX_CHANGE_BYTES: usize = 2 * MAX_EDIT_BYTES;
/// Unified diff text shared by every file of one approval preview.
pub const MAX_PREVIEW_BYTES: usize = 32 * 1024;

/// One entry of a multi-file change set, validated with every other entry before writing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileChange {
    /// Creates a new file and missing parents without replacing an entry.
    Create { path: String, text: String },
    /// Replaces a whole file at its current revision.
    Write {
        path: String,
        text: String,
        expected_revision: String,
    },
    /// Replacements match the original file, as in a single-file edit batch.
    Edit {
        path: String,
        expected_revision: String,
        edits: Vec<FileEdit>,
    },
    /// Moves a regular file to an absent destination in an existing parent.
    Move {
        path: String,
        destination: String,
        expected_revision: String,
    },
    /// Moves a regular file to the execution Node's Trash.
    Delete {
        path: String,
        expected_revision: String,
    },
}

impl FileChange {
    pub fn path(&self) -> &str {
        match self {
            Self::Create { path, .. }
            | Self::Write { path, .. }
            | Self::Edit { path, .. }
            | Self::Move { path, .. }
            | Self::Delete { path, .. } => path,
        }
    }

    pub fn destination(&self) -> Option<&str> {
        match self {
            Self::Move { destination, .. } => Some(destination),
            _ => None,
        }
    }

    pub fn expected_revision(&self) -> Option<&str> {
        match self {
            Self::Create { .. } => None,
            Self::Write {
                expected_revision, ..
            }
            | Self::Edit {
                expected_revision, ..
            }
            | Self::Move {
                expected_revision, ..
            }
            | Self::Delete {
                expected_revision, ..
            } => Some(expected_revision),
        }
    }

    pub fn kind(&self) -> ChangeKind {
        match self {
            Self::Create { .. } => ChangeKind::Added,
            Self::Write { .. } | Self::Edit { .. } => ChangeKind::Modified,
            Self::Move { .. } => ChangeKind::Renamed,
            Self::Delete { .. } => ChangeKind::Deleted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Renamed,
    Deleted,
}

/// Rejected means nothing was written; partial means some entries were written or uncertain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangesStatus {
    Applied,
    Partial,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Applied,
    Failed,
    /// The entry may have changed the file; it is never retried automatically.
    Unknown,
    NotAttempted,
}

/// Entries are written in order and stop at the first failure, without cross-file rollback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChanges {
    pub status: ChangesStatus,
    pub files: Vec<ChangedFile>,
}

impl FileChanges {
    /// The overall status follows the entries; uncertain effects are never reported as rejected.
    pub fn new(files: Vec<ChangedFile>) -> Self {
        let status = if files
            .iter()
            .all(|file| file.status == ChangeStatus::Applied)
        {
            ChangesStatus::Applied
        } else if files
            .iter()
            .any(|file| matches!(file.status, ChangeStatus::Applied | ChangeStatus::Unknown))
        {
            ChangesStatus::Partial
        } else {
            ChangesStatus::Rejected
        };
        Self { status, files }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    pub kind: ChangeKind,
    pub status: ChangeStatus,
    /// Full revision of the confirmed file content after an applied entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Bounded numbered regions around applied edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<FileExcerpt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::Fault>,
}

/// Approval facts for a validated change set, computed before any file is written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangePreview {
    pub files: Vec<PreviewFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
    /// Unified diff text within the shared preview budget; empty without a text change.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diff: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "edit_requires_a_revision",
                (|scenario: &'static str| {
                    let mut value = serde_json::json!({
                        "kind":"edit_file", "data": {"worktree":crate::WorktreeId::new(),"path":"file","old_text":"before","new_text":"after"}
                    });
                    assert!(
                        serde_json::from_value::<crate::Command>(value.clone()).is_err(),
                        "{scenario}"
                    );
                    value["data"]["expected_revision"] = "0".repeat(64).into();
                    let command: crate::Command = serde_json::from_value(value.clone()).unwrap();
                    assert_eq!(serde_json::to_value(&command).unwrap(), value, "{scenario}");
                    assert!(command.durable(), "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<crate::Command>(
                            serde_json::to_value(&command).unwrap()
                        )
                        .unwrap(),
                        command,
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "roundtrips_batch_edits_and_mutations",
                (|scenario: &'static str| {
                    let worktree = crate::WorktreeId::new();
                    for command in [
                        crate::Command::EditFile {
                            worktree,
                            path: "file".into(),
                            old_text: None,
                            new_text: None,
                            replace_all: false,
                            edits: Some(vec![FileEdit {
                                old_text: "old".into(),
                                new_text: "new".into(),
                                replace_all: true,
                            }]),
                            expected_revision: "a".repeat(64),
                        },
                        crate::Command::DeleteFile {
                            worktree,
                            path: "file".into(),
                            expected_revision: "a".repeat(64),
                        },
                        crate::Command::MoveFile {
                            worktree,
                            from: "file".into(),
                            to: "new".into(),
                            expected_revision: "a".repeat(64),
                        },
                    ] {
                        assert!(command.durable(), "{scenario}");
                        assert_eq!(
                            serde_json::from_value::<crate::Command>(
                                serde_json::to_value(&command).unwrap()
                            )
                            .unwrap(),
                            command,
                            "{scenario}"
                        );
                    }
                    for operation in [
                        crate::tool::Operation::DeleteFile,
                        crate::tool::Operation::MoveFile,
                        crate::tool::Operation::CreateDirectory,
                    ] {
                        assert!(!operation.read_only(), "{scenario}");
                        assert_eq!(
                            operation.action(),
                            Some(crate::plugin::Action::WriteFiles),
                            "{scenario}"
                        );
                    }
                }),
            ),
            (
                "change_sets_are_durable_typed_entries",
                (|scenario: &'static str| {
                    let revision = "a".repeat(64);
                    let value = serde_json::json!({
                        "kind":"apply_changes","data":{"worktree":crate::WorktreeId::new(),"changes":[
                            {"kind":"create","path":"new.rs","text":"fn main() {}\n"},
                            {"kind":"write","path":"README.md","text":"# Fixture\n","expected_revision":revision},
                            {"kind":"edit","path":"lib.rs","expected_revision":revision,"edits":[{"old_text":"a","new_text":"b"}]},
                            {"kind":"move","path":"old.rs","destination":"renamed.rs","expected_revision":revision},
                            {"kind":"delete","path":"dead.rs","expected_revision":revision}
                        ]}
                    });
                    let command: crate::Command = serde_json::from_value(value.clone()).unwrap();
                    assert!(command.durable(), "{scenario}");
                    assert_eq!(serde_json::to_value(&command).unwrap(), value, "{scenario}");
                    let crate::Command::ApplyChanges { changes, .. } = &command else {
                        panic!("{scenario}: change set expected")
                    };
                    assert_eq!(
                        changes.iter().map(FileChange::kind).collect::<Vec<_>>(),
                        [
                            ChangeKind::Added,
                            ChangeKind::Modified,
                            ChangeKind::Modified,
                            ChangeKind::Renamed,
                            ChangeKind::Deleted
                        ],
                        "{scenario}"
                    );
                    assert_eq!(changes[0].expected_revision(), None, "{scenario}");
                    assert_eq!(changes[3].destination(), Some("renamed.rs"), "{scenario}");
                    for invalid in [
                        serde_json::json!({"kind":"create","path":"a","text":"x","expected_revision":revision}),
                        serde_json::json!({"kind":"write","path":"a","text":"x"}),
                        serde_json::json!({"kind":"move","path":"a","expected_revision":revision}),
                        serde_json::json!({"kind":"rename","path":"a","destination":"b"}),
                    ] {
                        assert!(
                            serde_json::from_value::<FileChange>(invalid).is_err(),
                            "{scenario}"
                        );
                    }
                    let operation = crate::tool::Operation::ApplyChanges;
                    assert_eq!(
                        serde_json::to_value(operation).unwrap(),
                        "files.apply",
                        "{scenario}"
                    );
                    assert!(!operation.read_only(), "{scenario}");
                    assert_eq!(
                        operation.action(),
                        Some(crate::plugin::Action::WriteFiles),
                        "{scenario}"
                    );
                }),
            ),
            (
                "change_status_never_hides_uncertain_effects",
                (|scenario: &'static str| {
                    let file = |status| ChangedFile {
                        path: "file".into(),
                        destination: None,
                        kind: ChangeKind::Modified,
                        status,
                        revision: None,
                        size: None,
                        excerpt: None,
                        error: None,
                    };
                    for (statuses, expected) in [
                        (
                            vec![ChangeStatus::Applied, ChangeStatus::Applied],
                            ChangesStatus::Applied,
                        ),
                        (
                            vec![ChangeStatus::Applied, ChangeStatus::Failed],
                            ChangesStatus::Partial,
                        ),
                        (
                            vec![ChangeStatus::Unknown, ChangeStatus::NotAttempted],
                            ChangesStatus::Partial,
                        ),
                        (
                            vec![ChangeStatus::Failed, ChangeStatus::NotAttempted],
                            ChangesStatus::Rejected,
                        ),
                    ] {
                        let changes = FileChanges::new(statuses.into_iter().map(file).collect());
                        assert_eq!(changes.status, expected, "{scenario}");
                    }
                }),
            ),
            (
                "paged_reads_and_discovery_are_read_only",
                (|scenario: &'static str| {
                    for command in [
                        crate::Command::ReadFileLines {
                            worktree: crate::WorktreeId::new(),
                            path: "file".into(),
                            offset: 0,
                            limit: 200,
                            expected_revision: None,
                        },
                        crate::Command::FindFiles {
                            worktree: crate::WorktreeId::new(),
                            globs: vec!["**/*.rs".into()],
                            path: String::new(),
                            limit: MAX_FIND_MATCHES,
                            offset: 0,
                        },
                    ] {
                        assert!(!command.durable(), "{scenario}");
                        assert_eq!(
                            serde_json::from_value::<crate::Command>(
                                serde_json::to_value(&command).unwrap()
                            )
                            .unwrap(),
                            command,
                            "{scenario}"
                        );
                    }
                }),
            ),
            (
                "search_defaults_and_mode_payloads",
                (|scenario: &'static str| {
                    let options: FileSearch =
                        serde_json::from_value(serde_json::json!({"query":"needle"})).unwrap();
                    assert_eq!(options.limit, MAX_SEARCH_MATCHES, "{scenario}");
                    assert_eq!(options.mode, SearchMode::Content, "{scenario}");
                    assert_eq!(options.context, 0, "{scenario}");
                    assert!(options.path.is_empty(), "{scenario}");
                    assert!(
                        serde_json::from_value::<FileSearch>(
                            serde_json::json!({"query":"needle","mode":"unknown"})
                        )
                        .is_err(),
                        "{scenario}"
                    );
                    for (mode, field) in [
                        (SearchMode::Content, "matches"),
                        (SearchMode::Files, "paths"),
                        (SearchMode::Count, "counts"),
                    ] {
                        let result = SearchResults {
                            mode,
                            ..Default::default()
                        };
                        let value = serde_json::to_value(&result).unwrap();
                        assert_eq!(value[field], serde_json::json!([]), "{scenario}");
                        for other in ["matches", "paths", "counts"]
                            .into_iter()
                            .filter(|other| *other != field)
                        {
                            assert!(value.get(other).is_none(), "{scenario}");
                        }
                        assert_eq!(
                            serde_json::from_value::<SearchResults>(value).unwrap(),
                            result,
                            "{scenario}"
                        );
                    }
                }),
            ),
            (
                "download_requires_source_identity",
                (|scenario: &'static str| {
                    let mut value = serde_json::json!({
                        "stream": crate::StreamId::new(),
                        "worktree": crate::WorktreeId::new(),
                        "path": "file",
                        "size": 0,
                        "revision": "original"
                    });
                    assert!(
                        serde_json::from_value::<FileDownload>(value.clone()).is_err(),
                        "{scenario}"
                    );
                    value["stamp"] = "identity".into();
                    let download: FileDownload = serde_json::from_value(value).unwrap();
                    let command = crate::Command::TrashFile {
                        worktree: download.worktree,
                        path: download.path,
                        expected_revision: download.revision,
                        expected_stamp: download.stamp,
                    };
                    assert!(command.durable(), "{scenario}");
                    let encoded = serde_json::to_value(&command).unwrap();
                    assert_eq!(encoded["kind"], "trash_file", "{scenario}");
                    assert_eq!(
                        serde_json::from_value::<crate::Command>(encoded).unwrap(),
                        command,
                        "{scenario}"
                    );
                }),
            ),
            (
                "transfers_bind_both_roots",
                (|scenario: &'static str| {
                    for (case, command) in [
                        (
                            "copy_entry_to",
                            crate::Command::CopyEntryTo {
                                source: crate::WorktreeId::new(),
                                worktree: crate::WorktreeId::new(),
                                from: "资料/file".into(),
                                to: "copy".into(),
                            },
                        ),
                        (
                            "move_entry_to",
                            crate::Command::MoveEntryTo {
                                source: crate::WorktreeId::new(),
                                worktree: crate::WorktreeId::new(),
                                from: "file".into(),
                                to: "file".into(),
                            },
                        ),
                    ] {
                        assert!(command.durable(), "{scenario}: {case}: durability");
                        let encoded = serde_json::to_value(&command).unwrap();
                        assert_eq!(encoded["kind"], case, "{scenario}: {case}: wire kind");
                        assert_ne!(
                            encoded["data"]["source"], encoded["data"]["worktree"],
                            "{scenario}: {case}: distinct roots"
                        );
                        assert_eq!(
                            serde_json::from_value::<crate::Command>(encoded.clone()).unwrap(),
                            command,
                            "{scenario}: {case}: round trip"
                        );
                        let mut missing = encoded;
                        missing["data"].as_object_mut().unwrap().remove("source");
                        assert!(
                            serde_json::from_value::<crate::Command>(missing).is_err(),
                            "{scenario}: {case}: source required"
                        );
                    }
                }),
            ),
            (
                "first_page_requires_revision",
                (|scenario: &'static str| {
                    let command: crate::Command = serde_json::from_value(serde_json::json!({
                    "kind": "list_directory", "data": { "worktree": crate::WorktreeId::new(), "path": "" }
                }))
                .unwrap();
                    assert!(
                        matches!(command, crate::Command::ListDirectory { after: None, .. }),
                        "{scenario}"
                    );
                    assert!(!command.durable(), "{scenario}");
                    let mut value = serde_json::json!({
                        "path": "", "entries": [], "truncated": true, "unsupported_names": 0
                    });
                    assert!(
                        serde_json::from_value::<Directory>(value.clone()).is_err(),
                        "{scenario}"
                    );
                    value["revision"] = "directory".into();
                    let directory: Directory = serde_json::from_value(value).unwrap();
                    assert!(directory.next.is_none(), "{scenario}");
                    assert_eq!(directory.revision, "directory", "{scenario}");
                    assert!(directory.truncated, "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
