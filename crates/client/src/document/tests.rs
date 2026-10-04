use super::*;

#[test]
fn matches_content_scope_and_baseline() {
    let worktree = WorktreeId::new();
    let text = "document contents";
    let baseline = "original revision";
    let write = DocumentWrite {
        request: Request::new(sailry_protocol::NodeId([1; 32]), Command::Snapshot),
        spec: FileUploadSpec {
            worktree,
            path: "file.txt".into(),
            size: text.len() as u64,
            revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
            expected_revision: Some(baseline.into()),
        },
    };
    assert!(write.matches(worktree, "file.txt", text, baseline));
    assert!(!write.matches(WorktreeId::new(), "file.txt", text, baseline));
    assert!(!write.matches(worktree, "other.txt", text, baseline));
    assert!(!write.matches(worktree, "file.txt", "document CONTENTS", baseline));
    assert!(!write.matches(worktree, "file.txt", text, "another revision"));
}

#[test]
fn validates_text_bytes() {
    let mut text = "资料".repeat(MAX_DOCUMENT_BYTES / 6);
    text.push_str(&"x".repeat(MAX_DOCUMENT_BYTES - text.len()));
    assert!(validate_text(&text).is_ok());
    text.push('x');
    assert_eq!(
        validate_text(&text).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(validate_text("text\0binary").is_err());
    assert!(validate_text("").is_ok());
}
