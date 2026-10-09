use super::*;
use sailry_protocol::knowledge::Source;
use std::collections::BTreeMap;

fn directory() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn documents(scan: Scan) -> BTreeMap<String, SourceDoc> {
    scan.map(|entry| match entry.unwrap() {
        Entry::Document(document) => (document.path.clone(), document),
        Entry::Cached { .. } => panic!("full extraction unexpectedly reused a cache"),
        Entry::Skipped { path, reason, .. } => panic!("unexpected skipped source {path}: {reason}"),
    })
    .collect()
}

#[test]
fn retains_text_revisions_lines_and_locators() {
    let root = directory();
    std::fs::create_dir(root.path().join("notes")).unwrap();
    let text = "# Source\n中文 content 🙂\nlast line\n";
    std::fs::write(root.path().join("notes/note.md"), text).unwrap();
    let snapshot = documents(worktree(root.path(), "notes", CancellationToken::new()).unwrap());
    let document = &snapshot["notes/note.md"];
    assert_eq!(
        document.revision,
        blake3::hash(text.as_bytes()).to_hex().as_str()
    );
    assert_eq!(document.parts[0].text, text);
    assert_eq!(
        document.parts[0].position,
        Position::Lines { start: 1, end: 3 }
    );
    let timestamp = std::fs::metadata(root.path().join("notes/note.md"))
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert_eq!(document.modified_ms, timestamp);
    let path = root
        .path()
        .join("notes/note.md")
        .to_str()
        .unwrap()
        .to_owned();
    let selected = documents(
        controller(
            &Source::File { path: path.clone() },
            CancellationToken::new(),
        )
        .unwrap(),
    );
    assert_eq!(selected[&path].revision, document.revision);
    assert_eq!(selected[&path].parts[0].text, text);
    let selected = documents(
        controller(
            &Source::Directory {
                path: root.path().join("notes").to_str().unwrap().into(),
            },
            CancellationToken::new(),
        )
        .unwrap(),
    );
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[&path].revision, document.revision);
    std::fs::write(
        root.path().join("notes/note.md"),
        text.replace("last", "next"),
    )
    .unwrap();
    let updated =
        documents(worktree(root.path(), "notes/note.md", CancellationToken::new()).unwrap());
    assert_ne!(updated["notes/note.md"].revision, document.revision);
}

#[test]
fn honors_nested_ignores_and_protects_credentials() {
    let root = directory();
    for name in ["nested", ".git", ".ssh", "ignored"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    for (path, text) in [
        (".gitignore", "*.log\nignored/\nnested/*.txt\n"),
        ("nested/.gitignore", "!keep.txt\n"),
        ("keep.md", "kept source"),
        ("omit.log", "ignored source"),
        ("ignored/source.md", "ignored directory"),
        ("nested/keep.txt", "nested exception"),
        ("nested/omit.txt", "nested ignored"),
        (".git/config", "Git metadata"),
        (".ssh/id_ed25519", "private key"),
        (".env", "PRIVATE_TOKEN=fixture"),
        (".npmrc", "fixture credential"),
        ("certificate.pem", "fixture key"),
    ] {
        std::fs::write(root.path().join(path), text).unwrap();
    }
    let entries: Vec<_> = worktree(root.path(), "", CancellationToken::new())
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let paths: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Document(document) => Some(document.path.as_str()),
            _ => None,
        })
        .collect();
    assert!(paths.contains(&"keep.md"));
    assert!(paths.contains(&"nested/keep.txt"));
    for omitted in [
        "omit.log",
        "nested/omit.txt",
        "ignored/source.md",
        ".git/config",
        ".ssh/id_ed25519",
        ".env",
        ".npmrc",
        "certificate.pem",
    ] {
        assert!(!paths.contains(&omitted), "indexed {omitted}");
    }
    assert!(entries.iter().any(|entry| matches!(entry, Entry::Skipped { path, reason, excluded: true } if path == "omit.log" && reason.contains(".gitignore"))));
    for path in [".env", ".npmrc", ".git/config", ".ssh/id_ed25519"] {
        let source = Source::File {
            path: root.path().join(path).to_str().unwrap().into(),
        };
        assert!(
            matches!(controller(&source, CancellationToken::new()), Err(error) if error.code == ErrorCode::PermissionDenied)
        );
        assert!(
            matches!(worktree(root.path(), path, CancellationToken::new()), Err(error) if error.code == ErrorCode::PermissionDenied)
        );
    }
    let scoped = worktree(root.path(), "nested", CancellationToken::new()).unwrap();
    let paths: Vec<_> = scoped
        .filter_map(|entry| match entry.unwrap() {
            Entry::Document(document) => Some(document.path),
            _ => None,
        })
        .collect();
    assert!(paths.iter().any(|path| path == "nested/keep.txt"));
    assert!(!paths.iter().any(|path| path == "nested/omit.txt"));
}

#[test]
fn continues_after_partial_consumption() {
    let root = directory();
    for index in 0..300 {
        std::fs::write(
            root.path().join(format!("source-{index}.txt")),
            format!("source {index}"),
        )
        .unwrap();
    }
    let mut scan = worktree(root.path(), "", CancellationToken::new()).unwrap();
    let mut documents = BTreeMap::new();
    let first: Vec<_> = scan.by_ref().take(17).collect();
    for entry in first.into_iter().chain(scan) {
        let Entry::Document(document) = entry.unwrap() else {
            panic!("document expected")
        };
        assert!(documents.insert(document.path.clone(), document).is_none());
    }
    assert_eq!(documents.len(), 300);
    for index in 0..300 {
        assert_eq!(
            documents[&format!("source-{index}.txt")].parts[0].text,
            format!("source {index}")
        );
    }
}

#[test]
fn reports_binary_oversized_and_malformed_documents() {
    let root = directory();
    std::fs::write(root.path().join("binary.bin"), [0, 255, 0]).unwrap();
    std::fs::write(root.path().join("broken.pdf"), b"not a PDF").unwrap();
    let file = std::fs::File::create(root.path().join("large.md")).unwrap();
    file.set_len(crate::office::LIMIT as u64 + 1).unwrap();
    std::fs::write(root.path().join("valid.txt"), "source remains readable").unwrap();
    let mut omitted = BTreeMap::new();
    let mut completed = Vec::new();
    for entry in worktree(root.path(), "", CancellationToken::new()).unwrap() {
        match entry.unwrap() {
            Entry::Document(document) => completed.push(document.path),
            Entry::Cached { .. } => panic!("full extraction unexpectedly reused a cache"),
            Entry::Skipped { path, reason, .. } => {
                omitted.insert(path, reason);
            }
        }
    }
    assert_eq!(completed, ["valid.txt"]);
    assert_eq!(omitted.len(), 3);
    assert!(omitted["binary.bin"].contains("binary"));
    assert!(omitted["large.md"].contains("32 MiB"));
    assert!(omitted["broken.pdf"].contains("Office"));
}

#[test]
fn cancellation_stops_without_reporting_complete() {
    let root = directory();
    std::fs::write(root.path().join("source.txt"), "source").unwrap();
    let cancel = CancellationToken::new();
    let mut scan = worktree(root.path(), "", cancel.clone()).unwrap();
    cancel.cancel();
    assert_eq!(
        scan.next().unwrap().err().unwrap().code,
        ErrorCode::Cancelled
    );
    assert!(scan.next().is_none());
    assert!(
        matches!(worktree(root.path(), "", cancel), Err(error) if error.code == ErrorCode::Cancelled)
    );
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_roots_parents_files_and_ignores() {
    use std::os::unix::fs::symlink;
    let root = directory();
    let outside = directory();
    std::fs::write(outside.path().join("external.txt"), "outside source").unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    symlink(
        outside.path().join("external.txt"),
        root.path().join("external.txt"),
    )
    .unwrap();
    assert!(worktree(&root.path().join("linked"), "", CancellationToken::new()).is_err());
    assert!(worktree(root.path(), "linked/external.txt", CancellationToken::new()).is_err());
    assert!(worktree(root.path(), "../external.txt", CancellationToken::new()).is_err());
    assert!(
        controller(
            &Source::File {
                path: root.path().join("external.txt").to_str().unwrap().into()
            },
            CancellationToken::new()
        )
        .is_err()
    );
    let entries: Vec<_> = worktree(root.path(), "", CancellationToken::new())
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(
        entries.iter().all(
            |entry| matches!(entry, Entry::Skipped { reason, .. } if reason.contains("regular"))
        )
    );
    std::fs::create_dir(root.path().join("child")).unwrap();
    std::fs::write(
        root.path().join("child/source.txt"),
        "must not use external rules",
    )
    .unwrap();
    symlink(
        outside.path().join("external.txt"),
        root.path().join("child/.gitignore"),
    )
    .unwrap();
    let entries: Vec<_> = worktree(root.path(), "", CancellationToken::new())
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(entries.iter().any(|entry| matches!(entry, Entry::Skipped { path, reason, .. } if path == "child" && reason.contains(".gitignore"))));
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, Entry::Document(_)))
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("external.txt")).unwrap(),
        "outside source"
    );
}

#[test]
fn captures_four_document_formats_with_positions() {
    let root = directory();
    let fixtures: &[(&str, &[u8])] = &[
        (
            "report.pdf",
            include_bytes!("../../office/fixtures/report.pdf"),
        ),
        (
            "report.docx",
            include_bytes!("../../office/fixtures/report.docx"),
        ),
        (
            "book.xlsx",
            include_bytes!("../../office/fixtures/book.xlsx"),
        ),
        (
            "deck.pptx",
            include_bytes!("../../office/fixtures/deck.pptx"),
        ),
    ];
    for (name, bytes) in fixtures {
        std::fs::write(root.path().join(name), bytes).unwrap();
    }
    let snapshot = documents(worktree(root.path(), "", CancellationToken::new()).unwrap());
    for (name, bytes) in fixtures {
        assert_eq!(
            snapshot[*name].revision,
            blake3::hash(bytes).to_hex().as_str()
        );
        assert_eq!(std::fs::read(root.path().join(name)).unwrap(), *bytes);
    }
    let pdf = &snapshot["report.pdf"].parts;
    assert_eq!(pdf.len(), 2);
    assert_eq!(pdf[0].position, Position::Page { page: 1 });
    assert_eq!(pdf[1].position, Position::Page { page: 2 });
    assert!(pdf[0].text.contains("PDF acceptance"));
    assert!(pdf[1].text.contains("Second page"));
    let docx = &snapshot["report.docx"].parts;
    assert!(docx.iter().any(|part| part.position
        == Position::Section {
            name: "word/document.xml".into()
        }
        && part.text.contains("中文内容")));
    let workbook = &snapshot["book.xlsx"].parts;
    assert_eq!(
        workbook[0].position,
        Position::Cells {
            sheet: "预算".into(),
            start: "A1".into(),
            end: "B3".into()
        }
    );
    assert_eq!(
        workbook[1].position,
        Position::Cells {
            sheet: "备注".into(),
            start: "A1".into(),
            end: "A1".into()
        }
    );
    assert!(workbook[0].text.contains("20"));
    let slides = &snapshot["deck.pptx"].parts;
    assert_eq!(slides.len(), 4);
    assert_eq!(slides[0].position, Position::Slide { slide: 1 });
    assert_eq!(slides[1].position, Position::Slide { slide: 1 });
    assert_eq!(slides[2].position, Position::Slide { slide: 2 });
    assert_eq!(slides[3].position, Position::Slide { slide: 2 });
    assert!(slides[0].text.contains("中文幻灯片"));
    assert!(slides[2].text.contains("Second slide"));
}

#[test]
fn reuses_metadata_before_office_extraction() {
    let root = directory();
    let bytes = include_bytes!("../../office/fixtures/report.pdf");
    std::fs::write(root.path().join("report.pdf"), bytes).unwrap();
    let snapshot =
        documents(worktree(root.path(), "report.pdf", CancellationToken::new()).unwrap());
    let expected = snapshot["report.pdf"].stat;
    let mut scan = worktree(root.path(), "report.pdf", CancellationToken::new()).unwrap();
    let mut checked = 0;
    let entry = scan
        .next_cached(&mut |path, stat| {
            checked += 1;
            assert_eq!(path, "report.pdf");
            assert_eq!(stat, expected);
            Ok(true)
        })
        .unwrap()
        .unwrap();
    assert!(matches!(entry, Entry::Cached {path} if path == "report.pdf"));
    assert_eq!(checked, 1);
    assert!(scan.next().is_none());
    assert!(scan.complete());
    // Ordinary extraction still rereads and verifies the exact original content.
    let current = documents(worktree(root.path(), "report.pdf", CancellationToken::new()).unwrap());
    assert_eq!(
        current["report.pdf"].revision,
        snapshot["report.pdf"].revision
    );
    assert_eq!(
        current["report.pdf"].parts[0].position,
        Position::Page { page: 1 }
    );
    assert_eq!(
        current["report.pdf"].parts[0].text,
        snapshot["report.pdf"].parts[0].text
    );
}

#[test]
fn rejects_changes_and_cache_failures_during_inspection() {
    let root = directory();
    let path = root.path().join("source.txt");
    std::fs::write(&path, "original").unwrap();
    let mut scan = worktree(root.path(), "source.txt", CancellationToken::new()).unwrap();
    let error = scan
        .next_cached(&mut |_, _| {
            std::fs::write(&path, "a changed and longer source").unwrap();
            Ok(true)
        })
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    let mut scan = worktree(root.path(), "source.txt", CancellationToken::new()).unwrap();
    let error = scan
        .next_cached(&mut |_, _| Err(Fault::new(ErrorCode::Internal, "fixture cache failure")))
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(scan.next().is_none());
}

#[cfg(unix)]
#[test]
fn records_incomplete_ignored_subtree_without_following_links() {
    let root = directory();
    let outside = directory();
    std::fs::create_dir(root.path().join("child")).unwrap();
    std::fs::write(root.path().join("child/source.txt"), "retained source").unwrap();
    std::fs::write(outside.path().join("rules"), "external rules").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("rules"),
        root.path().join("child/.gitignore"),
    )
    .unwrap();
    let mut scan = worktree(root.path(), "", CancellationToken::new()).unwrap();
    let mut entries = Vec::new();
    let mut watched = Vec::new();
    while let Some(entry) =
        scan.next_watched(&mut |_, _| Ok(false), &mut |paths| watched.extend(paths))
    {
        entries.push(entry.unwrap());
    }
    assert!(entries.iter().any(|entry| matches!(entry, Entry::Skipped {path,reason,excluded:false} if path=="child" && reason.contains(".gitignore"))));
    assert!(!scan.complete());
    let repair = watched
        .iter()
        .find(|watch| watch.relative == "child")
        .unwrap();
    assert!(repair.accepts(".gitignore", false));
    assert!(!repair.accepts("source.txt", false));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("rules")).unwrap(),
        "external rules"
    );
}

#[test]
fn refuses_unauthorized_controller_source_kinds() {
    for source in [
        Source::Worktree {
            worktree: sailry_protocol::WorktreeId::new(),
            path: "".into(),
        },
        Source::Storage {
            scope: sailry_protocol::plugin::storage::Scope::Node,
            prefix: "prefix".into(),
        },
        Source::Directory {
            path: "relative".into(),
        },
    ] {
        assert!(controller(&source, CancellationToken::new()).is_err());
    }
}

#[test]
fn preserves_slide_order_independently_of_part_names() {
    use std::io::{Cursor, Write};
    let root = directory();
    let mut package = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, text) in [
        (
            "ppt/presentation.xml",
            r#"<presentation xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sldIdLst><sldId r:id="first"/><sldId r:id="second"/></sldIdLst></presentation>"#,
        ),
        (
            "ppt/_rels/presentation.xml.rels",
            r#"<Relationships><Relationship Id="first" Target="slides/slide9.xml"/><Relationship Id="second" Target="slides/slide3.xml"/></Relationships>"#,
        ),
        (
            "ppt/slides/slide9.xml",
            "<slide><p><t>First slide</t></p></slide>",
        ),
        (
            "ppt/slides/slide3.xml",
            "<slide><p><t>Second slide</t></p></slide>",
        ),
        (
            "ppt/slides/_rels/slide9.xml.rels",
            r#"<Relationships><Relationship Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide77.xml"/></Relationships>"#,
        ),
        (
            "ppt/notesSlides/notesSlide77.xml",
            "<notes><p><t>First notes</t></p></notes>",
        ),
    ] {
        package
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        package.write_all(text.as_bytes()).unwrap();
    }
    std::fs::write(
        root.path().join("ordered.pptx"),
        package.finish().unwrap().into_inner(),
    )
    .unwrap();
    let documents =
        documents(worktree(root.path(), "ordered.pptx", CancellationToken::new()).unwrap());
    let parts = &documents["ordered.pptx"].parts;
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].title, "ppt/slides/slide9.xml");
    assert_eq!(parts[0].text, "First slide");
    assert_eq!(parts[0].position, Position::Slide { slide: 1 });
    assert_eq!(parts[1].title, "ppt/notesSlides/notesSlide77.xml");
    assert_eq!(parts[1].text, "First notes");
    assert_eq!(parts[1].position, Position::Slide { slide: 1 });
    assert_eq!(parts[2].title, "ppt/slides/slide3.xml");
    assert_eq!(parts[2].position, Position::Slide { slide: 2 });
}

#[cfg(target_os = "macos")]
#[test]
fn preserves_controller_locators_through_system_aliases() {
    for (private, alias, temporary) in [
        ("/private/var", "/var", "/private/var/tmp"),
        ("/private/tmp", "/tmp", "/private/tmp"),
    ] {
        let root = tempfile::tempdir_in(temporary).unwrap();
        std::fs::write(root.path().join("source.txt"), "isolated source").unwrap();
        let alias = Path::new(alias).join(root.path().strip_prefix(private).unwrap());
        let snapshot = documents(
            controller(
                &Source::Directory {
                    path: alias.to_str().unwrap().into(),
                },
                CancellationToken::new(),
            )
            .unwrap(),
        );
        assert_eq!(snapshot.len(), 1);
        assert_eq!(
            snapshot[alias.join("source.txt").to_str().unwrap()].parts[0].text,
            "isolated source"
        );
    }
}
