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
    assert_eq!(candidates(&[]).unwrap(), ["AGENTS.md"]);
    assert_eq!(
        candidates(&[
            Scope::File("src/nested/first.rs".into()),
            Scope::Directory("src".into()),
            Scope::File("src/nested/second.rs".into()),
            Scope::Directory("tests".into()),
            Scope::Directory(String::new()),
        ])
        .unwrap(),
        [
            "AGENTS.md",
            "src/AGENTS.md",
            "src/nested/AGENTS.md",
            "tests/AGENTS.md",
        ]
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
    let directory = Fixture::new();
    std::fs::create_dir(directory.directory.path().join("AGENTS.md")).unwrap();
    let binary = Fixture::new();
    binary.write("AGENTS.md", [255]);
    let truncated = Fixture::new();
    truncated.write("AGENTS.md", vec![b'x'; sailry_protocol::MAX_FILE_BYTES + 1]);
    for fixture in [&directory, &binary, &truncated] {
        let error = fixture
            .load(&[], CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("AGENTS.md"));
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
