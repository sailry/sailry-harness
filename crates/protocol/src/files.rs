use serde::{Deserialize, Serialize};

pub const MAX_FILE_BYTES: usize = 128 * 1024;
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
pub struct FileSearch {
    pub query: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub globs: Vec<String>,
}

impl FileSearch {
    pub const MAX_GLOBS: usize = 64;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub matches: Vec<SearchMatch>,
    pub scanned_files: usize,
    pub skipped: usize,
    pub truncated: bool,
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
pub struct FileWritten {
    pub path: String,
    pub revision: String,
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_requires_source_identity() {
        let mut value = serde_json::json!({
            "stream": crate::StreamId::new(),
            "worktree": crate::WorktreeId::new(),
            "path": "file",
            "size": 0,
            "revision": "original"
        });
        assert!(serde_json::from_value::<FileDownload>(value.clone()).is_err());
        value["stamp"] = "identity".into();
        let download: FileDownload = serde_json::from_value(value).unwrap();
        let command = crate::Command::TrashFile {
            worktree: download.worktree,
            path: download.path,
            expected_revision: download.revision,
            expected_stamp: download.stamp,
        };
        assert!(command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["kind"], "trash_file");
        assert_eq!(
            serde_json::from_value::<crate::Command>(encoded).unwrap(),
            command
        );
    }

    #[test]
    fn copy_has_distinct_durable_identity() {
        let command = crate::Command::CopyEntryTo {
            source: crate::WorktreeId::new(),
            worktree: crate::WorktreeId::new(),
            from: "资料/file".into(),
            to: "copy".into(),
        };
        assert!(command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["kind"], "copy_entry_to");
        assert_ne!(encoded["data"]["source"], encoded["data"]["worktree"]);
        assert_eq!(
            serde_json::from_value::<crate::Command>(encoded.clone()).unwrap(),
            command
        );
        let mut missing = encoded;
        missing["data"].as_object_mut().unwrap().remove("source");
        assert!(serde_json::from_value::<crate::Command>(missing).is_err());
    }

    #[test]
    fn move_binds_both_roots() {
        let command = crate::Command::MoveEntryTo {
            source: crate::WorktreeId::new(),
            worktree: crate::WorktreeId::new(),
            from: "file".into(),
            to: "file".into(),
        };
        assert!(command.durable());
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["kind"], "move_entry_to");
        assert_ne!(encoded["data"]["source"], encoded["data"]["worktree"]);
        assert_eq!(
            serde_json::from_value::<crate::Command>(encoded.clone()).unwrap(),
            command
        );
        let mut missing = encoded;
        missing["data"].as_object_mut().unwrap().remove("source");
        assert!(serde_json::from_value::<crate::Command>(missing).is_err());
    }

    #[test]
    fn first_page_requires_revision() {
        let command: crate::Command = serde_json::from_value(serde_json::json!({
            "kind": "list_directory", "data": { "worktree": crate::WorktreeId::new(), "path": "" }
        }))
        .unwrap();
        assert!(matches!(
            command,
            crate::Command::ListDirectory { after: None, .. }
        ));
        assert!(!command.durable());
        let mut value = serde_json::json!({
            "path": "", "entries": [], "truncated": true, "unsupported_names": 0
        });
        assert!(serde_json::from_value::<Directory>(value.clone()).is_err());
        value["revision"] = "directory".into();
        let directory: Directory = serde_json::from_value(value).unwrap();
        assert!(directory.next.is_none());
        assert_eq!(directory.revision, "directory");
        assert!(directory.truncated);
    }
}
