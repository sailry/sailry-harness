use super::*;

#[tokio::test]
async fn reads_complete_sources_and_rejects_stale_ranges() {
    let fixture = Fixture::empty().await;
    let prefix = format!("// 中文🙂 {}\n", "x".repeat(140_000));
    let before = format!("{prefix}pub fn alpha_symbol() {{}}\n");
    let after = format!("{prefix}pub fn omega_symbol() {{}}\n");
    assert!(before.len() > sailry_protocol::MAX_FILE_BYTES);
    assert!(before.len() < code_index::MAX_FILE_BYTES);
    assert_eq!(before.len(), after.len());
    for client in fixture.clients() {
        std::fs::write(fixture.root.join("large.rs"), &before).unwrap();
        let outline = client
            .outline_code(fixture.worktree, "large.rs")
            .await
            .unwrap();
        assert_eq!(outline.status, code_index::Status::Parsed);
        assert_eq!(
            outline.file_revision.as_deref(),
            Some(revision(&before).as_str())
        );
        let symbol = outline
            .symbols
            .iter()
            .find(|item| item.name == "alpha_symbol")
            .expect("the declaration after the preview limit is indexed");
        assert!(symbol.range.start_byte as usize > sailry_protocol::MAX_FILE_BYTES);
        assert_eq!(symbol.file_revision, revision(&before));
        let lines = client
            .read_file_lines(
                fixture.worktree,
                "large.rs",
                symbol.range.start_line as usize,
                1,
                Some(symbol.file_revision.clone()),
            )
            .await
            .unwrap();
        assert_eq!(lines.revision, symbol.file_revision);
        assert_eq!(lines.lines.len(), 1);
        assert!(lines.lines[0].text.contains("alpha_symbol"));
        std::fs::write(fixture.root.join("large.rs"), &after).unwrap();
        assert_eq!(
            client
                .read_file_lines(
                    fixture.worktree,
                    "large.rs",
                    symbol.range.start_line as usize,
                    1,
                    Some(symbol.file_revision.clone()),
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let updated = client
            .outline_code(fixture.worktree, "large.rs")
            .await
            .unwrap();
        assert!(updated.index_revision > outline.index_revision);
        assert_eq!(
            updated.file_revision.as_deref(),
            Some(revision(&after).as_str())
        );
        assert!(
            updated
                .symbols
                .iter()
                .any(|item| item.name == "omega_symbol")
        );
        assert!(
            updated
                .symbols
                .iter()
                .all(|item| item.name != "alpha_symbol")
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn reports_oversized_sources_without_partial_symbols() {
    let fixture = Fixture::empty().await;
    let source = format!(
        "pub fn omitted_symbol() {{}}\n{}",
        " ".repeat(code_index::MAX_FILE_BYTES)
    );
    std::fs::write(fixture.root.join("oversized.rs"), source).unwrap();
    for client in fixture.clients() {
        let outline = client
            .outline_code(fixture.worktree, "oversized.rs")
            .await
            .unwrap();
        assert_eq!(outline.status, code_index::Status::TooLarge);
        assert!(outline.partial);
        assert!(outline.truncated);
        assert!(outline.file_revision.is_none());
        assert!(outline.symbols.is_empty());
    }
    fixture.close().await;
}
