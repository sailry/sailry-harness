use super::*;
use sailry_protocol::ErrorCode;

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

    fn write(&self, text: impl AsRef<[u8]>) {
        std::fs::write(self.directory.path().join("instructions.md"), text).unwrap();
    }

    async fn load(&self, stop: CancellationToken) -> Result<String, Fault> {
        load_from(
            &self.files,
            &self.directory.path().canonicalize().unwrap(),
            WorktreeId::new(),
            &stop,
        )
        .await
    }
}

#[tokio::test]
async fn ignores_missing_and_empty_files() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .load(CancellationToken::new())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.directory.path().join("instructions.md").exists());
    fixture.write(" \n\t");
    assert!(
        fixture
            .load(CancellationToken::new())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn preserves_text_and_authority() {
    let fixture = Fixture::new();
    let text = "Keep the requested Unicode content 中文 🙂\nUse concise answers\n";
    fixture.write(text);
    let instruction = fixture.load(CancellationToken::new()).await.unwrap();
    assert!(instruction.ends_with(text));
    assert!(instruction.contains("Source: instructions.md\n"));
    assert!(
        instruction
            .contains("work mode, tool permissions and delegated scope remain authoritative")
    );
    assert!(!instruction.contains(fixture.directory.path().to_str().unwrap()));
    fixture.write("Next turn's preference");
    let next = fixture.load(CancellationToken::new()).await.unwrap();
    assert!(next.contains("Next turn's preference"));
    assert!(!next.contains(text));
    assert!(instruction.ends_with(text));
    let maximum = "x".repeat(sailry_protocol::MAX_FILE_BYTES);
    fixture.write(&maximum);
    assert!(
        fixture
            .load(CancellationToken::new())
            .await
            .unwrap()
            .ends_with(&maximum)
    );
}

#[tokio::test]
async fn rejects_unreadable_and_partial_files() {
    let directory = Fixture::new();
    std::fs::create_dir(directory.directory.path().join("instructions.md")).unwrap();
    let binary = Fixture::new();
    binary.write([255]);
    let nul = Fixture::new();
    nul.write("Binary\0content");
    let truncated = Fixture::new();
    truncated.write(vec![b'x'; sailry_protocol::MAX_FILE_BYTES + 1]);
    for fixture in [&directory, &binary, &nul, &truncated] {
        let error = fixture.load(CancellationToken::new()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("instructions.md"));
    }
    assert!(
        truncated
            .load(CancellationToken::new())
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
    outside.write("Outside preferences");
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(
        outside.directory.path().join("instructions.md"),
        fixture.directory.path().join("instructions.md"),
    )
    .unwrap();
    let error = fixture.load(CancellationToken::new()).await.unwrap_err();
    assert_ne!(error.code, ErrorCode::NotFound);
    assert!(!error.message.contains("Outside preferences"));
}

#[tokio::test]
async fn stops_cancelled_reads() {
    let fixture = Fixture::new();
    fixture.write("Personal preferences");
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        fixture.load(stop).await.unwrap_err().code,
        ErrorCode::Cancelled
    );
}
