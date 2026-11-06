//! Change-set results state each file's outcome and edited regions, so the model
//! can continue without re-reading files or guessing which entries were written.
use super::*;
use sailry_protocol::{ChangeKind, ChangeStatus, ChangesStatus, FileChanges};

pub(super) fn project(original: &Value) -> Option<String> {
    let changes: FileChanges = serde_json::from_value(original.get("data")?.clone()).ok()?;
    let applied = changes
        .files
        .iter()
        .any(|file| file.status == ChangeStatus::Applied);
    let mut text = String::from(match changes.status {
        ChangesStatus::Applied => "All changes applied\n",
        ChangesStatus::Rejected => "No changes applied; no file content was changed\n",
        ChangesStatus::Partial if applied => {
            "Changes partially applied; writing stopped at the first failure and earlier files were not rolled back\n"
        }
        ChangesStatus::Partial => "Outcome unknown; writing stopped at an uncertain entry\n",
    });
    for file in &changes.files {
        match &file.destination {
            Some(destination) => text.push_str(&format!("{} -> {destination}: ", file.path)),
            None => text.push_str(&format!("{}: ", file.path)),
        }
        match file.status {
            ChangeStatus::Applied => {
                text.push_str(match file.kind {
                    ChangeKind::Added => "created",
                    ChangeKind::Modified => "modified",
                    ChangeKind::Renamed => "moved",
                    ChangeKind::Deleted => "moved to Trash",
                });
                if let Some(size) = file.size {
                    text.push_str(&format!("; {size} bytes"));
                }
                if let Some(revision) = &file.revision {
                    text.push_str(&format!("; revision {revision}"));
                }
                text.push('\n');
            }
            ChangeStatus::Failed | ChangeStatus::Unknown => {
                let error = file.error.as_ref();
                let code = error
                    .and_then(|error| serde_json::to_value(error.code).ok())
                    .and_then(|code| code.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let reason = error.map_or("", |error| error.message.as_str());
                text.push_str(if file.status == ChangeStatus::Unknown {
                    "outcome unknown"
                } else {
                    "failed"
                });
                text.push_str(&format!(" ({code}): {reason}\n"));
                if file.status == ChangeStatus::Unknown {
                    text.push_str("Inspect this file before retrying\n");
                }
            }
            ChangeStatus::NotAttempted => text.push_str("not attempted\n"),
        }
        if let Some(excerpt) = &file.excerpt {
            edits::render(&mut text, excerpt);
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{ChangedFile, ErrorCode, Fault, FileExcerpt, FileHunk, FileLine};

    fn file(path: &str, kind: ChangeKind, status: ChangeStatus) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            destination: None,
            kind,
            status,
            revision: None,
            size: None,
            excerpt: None,
            error: None,
        }
    }

    #[test]
    fn states_each_file_outcome() {
        let mut edited = file("src/lib.rs", ChangeKind::Modified, ChangeStatus::Applied);
        edited.revision = Some("a".repeat(64));
        edited.size = Some(20);
        edited.excerpt = Some(FileExcerpt {
            hunks: vec![FileHunk {
                lines: vec![FileLine {
                    number: 3,
                    text: "fn changed() {}\n".into(),
                }],
                shortened: false,
            }],
            omitted: 0,
        });
        let mut moved = file("old.rs", ChangeKind::Renamed, ChangeStatus::Failed);
        moved.destination = Some("new.rs".into());
        moved.error = Some(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed outside this operation",
        ));
        let changes = FileChanges::new(vec![
            edited,
            moved,
            file("dead.rs", ChangeKind::Deleted, ChangeStatus::NotAttempted),
        ]);
        let text = project(&json!({"kind":"file_changes","data":changes})).unwrap();
        assert!(text.starts_with("Changes partially applied"), "{text}");
        assert!(text.contains(&format!(
            "src/lib.rs: modified; 20 bytes; revision {}\nLines 3–3\n3\tfn changed() {{}}\n",
            "a".repeat(64)
        )));
        assert!(text.contains(
            "old.rs -> new.rs: failed (revision_conflict): file changed outside this operation\n"
        ));
        assert!(text.ends_with("dead.rs: not attempted\n"));
    }

    #[test]
    fn marks_uncertain_entries_for_inspection() {
        let mut uncertain = file("new.rs", ChangeKind::Added, ChangeStatus::Unknown);
        uncertain.error = Some(Fault::new(ErrorCode::OutcomeUnknown, "unconfirmed"));
        let text =
            project(&json!({"kind":"file_changes","data":FileChanges::new(vec![uncertain])}))
                .unwrap();
        assert!(text.starts_with("Outcome unknown;"), "{text}");
        assert!(text.contains("new.rs: outcome unknown (outcome_unknown): unconfirmed\nInspect this file before retrying\n"));
    }

    #[test]
    fn keeps_applied_entries_before_uncertain_one() {
        let applied = file("first.rs", ChangeKind::Modified, ChangeStatus::Applied);
        let mut uncertain = file("second.rs", ChangeKind::Modified, ChangeStatus::Unknown);
        uncertain.error = Some(Fault::new(ErrorCode::OutcomeUnknown, "unconfirmed"));
        let changes = FileChanges::new(vec![
            applied,
            uncertain,
            file("third.rs", ChangeKind::Deleted, ChangeStatus::NotAttempted),
        ]);
        assert_eq!(changes.status, ChangesStatus::Partial);
        let text = project(&json!({"kind":"file_changes","data":changes})).unwrap();
        assert!(text.starts_with("Changes partially applied"), "{text}");
        assert!(text.contains("second.rs: outcome unknown (outcome_unknown): unconfirmed\nInspect this file before retrying\nthird.rs: not attempted\n"));
    }

    #[test]
    fn rejection_does_not_claim_untouched_folders() {
        let mut failed = file("a.rs", ChangeKind::Added, ChangeStatus::Failed);
        failed.error = Some(Fault::new(ErrorCode::Conflict, "exists"));
        let text =
            project(&json!({"kind":"file_changes","data":FileChanges::new(vec![failed])})).unwrap();
        assert!(
            text.starts_with("No changes applied; no file content was changed\n"),
            "{text}"
        );
    }
}
