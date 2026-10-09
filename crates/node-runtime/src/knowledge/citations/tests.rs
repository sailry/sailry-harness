use super::*;
use sailry_protocol::KnowledgeId;

fn directory() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn citation(path: &str, bytes: &[u8], position: Position) -> Citation {
    let mut citation = Citation {
        collection: KnowledgeId::new(),
        chunk: "0".repeat(64),
        revision: blake3::hash(bytes).to_hex().to_string(),
        path: path.into(),
        position,
        link: String::new(),
    };
    citation.link = citation.url();
    citation
}

fn definition(source: Source) -> Definition {
    Definition {
        name: "Source".into(),
        source,
        model: None,
        globs: Vec::new(),
        tags: Vec::new(),
        shared_with: Vec::new(),
    }
}

#[test]
fn rereads_exact_worktree_lines_and_revision() {
    let root = directory();
    std::fs::create_dir(root.path().join("notes")).unwrap();
    let bytes = b"first\nsecond line\nlast\n";
    std::fs::write(root.path().join("notes/source.md"), bytes).unwrap();
    let definition = definition(Source::Worktree {
        worktree: WorktreeId::new(),
        path: "notes".into(),
    });
    let citation = citation(
        "notes/source.md",
        bytes,
        Position::Lines { start: 2, end: 3 },
    );
    assert_eq!(
        context(
            &definition,
            Some(root.path()),
            &citation,
            CancellationToken::new()
        )
        .unwrap(),
        "second line\nlast"
    );
    std::fs::write(
        root.path().join("notes/source.md"),
        b"first\nchanged\nlast\n",
    )
    .unwrap();
    assert_eq!(
        context(
            &definition,
            Some(root.path()),
            &citation,
            CancellationToken::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    std::fs::remove_file(root.path().join("notes/source.md")).unwrap();
    assert_eq!(
        context(
            &definition,
            Some(root.path()),
            &citation,
            CancellationToken::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn scopes_sources_and_current_globs() {
    let root = directory();
    std::fs::create_dir(root.path().join("notes")).unwrap();
    std::fs::write(root.path().join("notes/source.txt"), b"private source").unwrap();
    let path = root
        .path()
        .join("notes/source.txt")
        .to_str()
        .unwrap()
        .to_owned();
    let citation = citation(
        &path,
        b"private source",
        Position::Lines { start: 1, end: 1 },
    );
    let mut definition = definition(Source::Directory {
        path: root.path().join("notes").to_str().unwrap().into(),
    });
    assert_eq!(
        context(&definition, None, &citation, CancellationToken::new()).unwrap(),
        "private source"
    );
    definition.globs = vec!["**/*.md".into()];
    assert_eq!(
        context(&definition, None, &citation, CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    definition.globs.clear();
    std::fs::write(root.path().join("notes/.gitignore"), "source.txt\n").unwrap();
    assert_eq!(
        context(&definition, None, &citation, CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    definition.source = Source::Directory {
        path: root.path().join("other").to_str().unwrap().into(),
    };
    assert_eq!(
        context(&definition, None, &citation, CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    definition.source = Source::File {
        path: format!("{path}-different"),
    };
    assert_eq!(
        context(&definition, None, &citation, CancellationToken::new())
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn resolves_office_positions_from_actual_sources() {
    let root = directory();
    let cases: &[(&str, &[u8], Position, &str)] = &[
        (
            "report.pdf",
            include_bytes!("../../office/fixtures/report.pdf"),
            Position::Page { page: 2 },
            "Second page",
        ),
        (
            "book.xlsx",
            include_bytes!("../../office/fixtures/book.xlsx"),
            Position::Cells {
                sheet: "预算".into(),
                start: "A1".into(),
                end: "B3".into(),
            },
            "20",
        ),
        (
            "deck.pptx",
            include_bytes!("../../office/fixtures/deck.pptx"),
            Position::Slide { slide: 2 },
            "Second slide",
        ),
        (
            "report.docx",
            include_bytes!("../../office/fixtures/report.docx"),
            Position::Section {
                name: "word/document.xml".into(),
            },
            "中文内容",
        ),
    ];
    for (name, bytes, position, expected) in cases {
        let path = root.path().join(name).to_str().unwrap().to_owned();
        std::fs::write(&path, bytes).unwrap();
        let definition = definition(Source::File { path: path.clone() });
        let mut citation = citation(&path, bytes, position.clone());
        assert!(
            context(&definition, None, &citation, CancellationToken::new())
                .unwrap()
                .contains(expected)
        );
        citation.position = Position::Page { page: 900 };
        assert_eq!(
            context(&definition, None, &citation, CancellationToken::new())
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
}

#[cfg(unix)]
#[test]
fn rejects_replaced_symlink_sources() {
    let root = directory();
    let outside = directory();
    let path = root.path().join("source.txt").to_str().unwrap().to_owned();
    std::fs::write(&path, b"original").unwrap();
    std::fs::write(outside.path().join("source.txt"), b"private sibling").unwrap();
    let definition = definition(Source::File { path: path.clone() });
    let citation = citation(&path, b"original", Position::Lines { start: 1, end: 1 });
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside.path().join("source.txt"), &path).unwrap();
    assert!(context(&definition, None, &citation, CancellationToken::new()).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("source.txt")).unwrap(),
        "private sibling"
    );
}

#[test]
fn pages_verified_context_without_losing_utf8() {
    let text = "前🙂tail repeated source context";
    let citation = citation(
        "notes/source.md",
        text.as_bytes(),
        Position::Lines { start: 1, end: 1 },
    );
    let mut request = CitationRequest {
        collection: citation.collection,
        chunk: citation.chunk.clone(),
        revision: citation.revision.clone(),
        offset: 0,
        limit: 7,
    };
    let mut combined = String::new();
    loop {
        let output = page(
            index::Record {
                citation: citation.clone(),
                title: "Source".into(),
            },
            text,
            &request,
            None,
        )
        .unwrap();
        assert!(output.text.len() <= request.limit as usize);
        assert_eq!(output.offset, request.offset);
        combined.push_str(&output.text);
        match output.next_offset {
            Some(offset) => request.offset = offset,
            None => break,
        }
    }
    assert_eq!(combined, text);
    request.offset = 1;
    assert_eq!(
        page(
            index::Record {
                citation,
                title: "Source".into()
            },
            text,
            &request,
            None
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn cancels_before_source_acquisition() {
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(check_cancel(&stop).unwrap_err().code, ErrorCode::Cancelled);
}
