use super::*;

struct Fixture {
    directory: tempfile::TempDir,
    files: Files,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            files: Files::new(),
        }
    }

    fn write(&self, path: &str, content: impl AsRef<[u8]>) {
        let path = self.directory.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    async fn load(&self, scopes: &[Scope], stop: CancellationToken) -> Result<String, Fault> {
        load_from(
            &self.files,
            self.directory.path().canonicalize().unwrap(),
            WorktreeId::new(),
            candidates(scopes)?,
            &stop,
        )
        .await
    }
}

#[test]
fn orders_and_deduplicates_ancestors() {
    assert_eq!(candidates(&[]).unwrap(), [""]);
    assert_eq!(
        candidates(&[
            Scope::File("src/nested/first.rs".into()),
            Scope::Directory("src".into()),
            Scope::File("src/nested/second.rs".into()),
            Scope::Directory("tests".into()),
            Scope::Directory(String::new()),
        ])
        .unwrap(),
        ["", "src", "src/nested", "tests",]
    );
}

#[test]
fn rejects_unsafe_scopes() {
    for scope in [
        Scope::File(String::new()),
        Scope::File("../outside/secret.rs".into()),
        Scope::Directory("/outside".into()),
        Scope::Directory("src/../outside".into()),
        Scope::File(r"C:\outside\secret.rs".into()),
    ] {
        assert_eq!(
            candidates(&[scope]).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[tokio::test]
async fn loads_only_relevant_guides() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "Root rule");
    fixture.write("资料/AGENTS.md", "Directory rule");
    fixture.write("资料/nested/AGENTS.md", "Nested rule");
    fixture.write("资料/sibling/AGENTS.md", "Sibling rule");
    fixture.write("资料/nested/deeper/AGENTS.md", "Descendant rule");
    fixture.write("other/AGENTS.md", "Other rule");
    let instruction = fixture
        .load(
            &[Scope::File("资料/nested/module.rs".into())],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(
        instruction.contains("These guides apply only within their stated project-relative scope")
    );
    assert!(instruction.contains(
        "Explicit user instructions and this turn's mode and permissions remain authoritative"
    ));
    for (path, scope) in [
        ("AGENTS.md", "."),
        ("资料/AGENTS.md", "资料"),
        ("资料/nested/AGENTS.md", "资料/nested"),
    ] {
        assert!(instruction.contains(&format!("Guide: {path}\nScope: {scope}\n")));
    }
    assert!(instruction.find("Root rule").unwrap() < instruction.find("Directory rule").unwrap());
    assert!(instruction.find("Directory rule").unwrap() < instruction.find("Nested rule").unwrap());
    for excluded in ["Sibling rule", "Descendant rule", "Other rule"] {
        assert!(!instruction.contains(excluded));
    }
    assert!(!instruction.contains(fixture.directory.path().to_str().unwrap()));
    let root = fixture.load(&[], CancellationToken::new()).await.unwrap();
    assert!(root.contains("Root rule"));
    assert!(!root.contains("Directory rule"));
}

#[tokio::test]
async fn skips_missing_and_empty_guides() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .load(&[], CancellationToken::new())
            .await
            .unwrap()
            .is_empty()
    );
    fixture.write("AGENTS.md", " \n\t");
    fixture.write("CLAUDE.md", "Suppressed root fallback");
    fixture.write("GEMINI.md", "Suppressed root fallback");
    fixture.write("src/CLAUDE.md", "\n");
    fixture.write("src/GEMINI.md", "Suppressed directory fallback");
    fixture.write("src/nested/AGENTS.md", "Nested rule");
    let instruction = fixture
        .load(
            &[Scope::Directory("src/nested".into())],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(instruction.contains("Guide: src/nested/AGENTS.md\n"));
    assert!(!instruction.contains("Guide: AGENTS.md\n"));
    assert!(!instruction.contains("Guide: src/AGENTS.md\n"));
    assert!(!instruction.contains("Suppressed"));
}

#[tokio::test]
async fn selects_first_existing_guide() {
    let fixture = Fixture::new();
    for filename in FILENAMES {
        fixture.write(filename, format!("Selected {filename}"));
    }
    for (index, filename) in FILENAMES.iter().enumerate() {
        let instruction = fixture.load(&[], CancellationToken::new()).await.unwrap();
        assert!(instruction.contains(&format!("Guide: {filename}\nScope: .\nSelected {filename}")));
        for ignored in &FILENAMES[index + 1..] {
            assert!(!instruction.contains(&format!("Selected {ignored}")));
        }
        std::fs::remove_file(fixture.directory.path().join(filename)).unwrap();
    }
    assert!(
        fixture
            .load(&[], CancellationToken::new())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn chooses_independently_in_each_directory() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "Root rule");
    fixture.write("CLAUDE.md", "Suppressed root fallback");
    fixture.write("src/CLAUDE.md", "Directory rule");
    fixture.write("src/GEMINI.md", "Suppressed directory fallback");
    fixture.write("src/nested/GEMINI.md", "Nested rule");
    fixture.write("src/other/GEMINI.md", "Unrelated rule");
    let instruction = fixture
        .load(
            &[Scope::File("src/nested/main.rs".into())],
            CancellationToken::new(),
        )
        .await
        .unwrap();
    for (path, scope) in [
        ("AGENTS.md", "."),
        ("src/CLAUDE.md", "src"),
        ("src/nested/GEMINI.md", "src/nested"),
    ] {
        assert!(instruction.contains(&format!("Guide: {path}\nScope: {scope}\n")));
    }
    assert!(instruction.find("Root rule").unwrap() < instruction.find("Directory rule").unwrap());
    assert!(instruction.find("Directory rule").unwrap() < instruction.find("Nested rule").unwrap());
    assert!(!instruction.contains("Suppressed"));
    assert!(!instruction.contains("Unrelated rule"));
}

#[tokio::test]
async fn reloads_changed_guides() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "First rule");
    let first = fixture.load(&[], CancellationToken::new()).await.unwrap();
    assert!(first.contains("First rule"));
    fixture.write("AGENTS.md", "Second rule");
    let second = fixture.load(&[], CancellationToken::new()).await.unwrap();
    assert!(second.contains("Second rule"));
    assert!(!second.contains("First rule"));
}

#[tokio::test]
async fn rejects_unreadable_and_partial_guides() {
    for filename in FILENAMES {
        let directory = Fixture::new();
        std::fs::create_dir(directory.directory.path().join(filename)).unwrap();
        let binary = Fixture::new();
        binary.write(filename, [255]);
        let nul = Fixture::new();
        nul.write(filename, "Binary\0content");
        let truncated = Fixture::new();
        truncated.write(filename, vec![b'x'; sailry_protocol::MAX_FILE_BYTES + 1]);
        for fixture in [&directory, &binary, &nul, &truncated] {
            for fallback in FILENAMES
                .iter()
                .skip_while(|name| **name != filename)
                .skip(1)
            {
                fixture.write(fallback, "Must not bypass a failed guide");
            }
            let error = fixture
                .load(&[], CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert!(error.message.contains(filename));
        }
        assert!(
            truncated
                .load(&[], CancellationToken::new())
                .await
                .unwrap_err()
                .message
                .contains("exceed the file read limit")
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_outside_links() {
    let outside = Fixture::new();
    outside.write("AGENTS.md", "Outside rule");
    for directory in [false, true] {
        let fixture = Fixture::new();
        let (target, name, scopes) = if directory {
            (
                outside.directory.path().to_path_buf(),
                "linked",
                vec![Scope::File("linked/module.rs".into())],
            )
        } else {
            (
                outside.directory.path().join("AGENTS.md"),
                "AGENTS.md",
                Vec::new(),
            )
        };
        std::os::unix::fs::symlink(target, fixture.directory.path().join(name)).unwrap();
        let error = fixture
            .load(&scopes, CancellationToken::new())
            .await
            .unwrap_err();
        assert_ne!(error.code, ErrorCode::NotFound);
        assert!(!error.message.contains("Outside rule"));
    }
    for filename in FILENAMES {
        let fixture = Fixture::new();
        std::os::unix::fs::symlink(
            outside.directory.path().join("AGENTS.md"),
            fixture.directory.path().join(filename),
        )
        .unwrap();
        for fallback in FILENAMES
            .iter()
            .skip_while(|name| **name != filename)
            .skip(1)
        {
            fixture.write(fallback, "Must not bypass an outside link");
        }
        let error = fixture
            .load(&[], CancellationToken::new())
            .await
            .unwrap_err();
        assert_ne!(error.code, ErrorCode::NotFound);
        assert!(!error.message.contains("Outside rule"));
    }
}

#[tokio::test]
async fn stops_cancelled_reads() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "Root rule");
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        fixture.load(&[], stop).await.unwrap_err().code,
        ErrorCode::Cancelled
    );
}
