use super::*;
use std::path::PathBuf;

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    repository: Repository,
    revision: String,
    head: String,
    bytes: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let repository = Repository::init(&root).unwrap();
        std::fs::write(root.join("source.txt"), "before\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("source.txt")).unwrap();
        index.write().unwrap();
        let tree = index.write_tree().unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        let head = repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Initial",
                &repository.find_tree(tree).unwrap(),
                &[],
            )
            .unwrap()
            .to_string();
        let bytes = std::fs::read(root.join(".git/index")).unwrap();
        let revision = digest(Some(&bytes));
        std::fs::write(root.join("source.txt"), "after\n").unwrap();
        Self {
            _directory: directory,
            root,
            repository,
            revision,
            head,
            bytes,
        }
    }

    fn edit(
        &self,
        transform: impl FnOnce(&str) -> Result<String, Fault>,
    ) -> Result<GitIndex, Fault> {
        edit(
            self.root.as_path(),
            "source.txt",
            &self.revision,
            Some(&self.head),
            RequestId::new(),
            blake3::hash(b"after\n").to_hex().as_str(),
            transform,
        )
    }

    fn clean(&self) {
        assert!(!self.root.join(".git/index.lock").exists());
        assert!(
            !std::fs::read_dir(self.root.join(".git"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("sailry-index-"))
        );
    }
}

#[test]
fn refuses_racing_index_publication() {
    let fixture = Fixture::new();
    let alternate = fixture.root.join("alternate-index");
    std::fs::write(&alternate, &fixture.bytes).unwrap();
    let mut index = Index::open(&alternate).unwrap();
    let blob = fixture.repository.blob(b"Manual staged content\n").unwrap();
    index
        .add(&entry_record("manual.txt", 0o100644, blob))
        .unwrap();
    index.write().unwrap();
    let raced = std::fs::read(&alternate).unwrap();
    let error = fixture
        .edit(|before| {
            assert_eq!(before, "before\n");
            std::fs::write(fixture.root.join(".git/index"), &raced).unwrap();
            Ok("after\n".into())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(
        std::fs::read(fixture.root.join(".git/index")).unwrap(),
        raced
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
        "after\n"
    );
    fixture.clean();
}

#[test]
fn refuses_racing_source_and_head() {
    for source in [false, true] {
        let fixture = Fixture::new();
        let error = fixture
            .edit(|_| {
                if source {
                    std::fs::write(fixture.root.join("source.txt"), "Manual replacement\n")
                        .unwrap();
                } else {
                    let parent = fixture
                        .repository
                        .find_commit(Oid::from_str(&fixture.head).unwrap())
                        .unwrap();
                    let signature =
                        git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
                    fixture
                        .repository
                        .commit(
                            Some("HEAD"),
                            &signature,
                            &signature,
                            "Concurrent commit",
                            &parent.tree().unwrap(),
                            &[&parent],
                        )
                        .unwrap();
                }
                Ok("after\n".into())
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(
            std::fs::read(fixture.root.join(".git/index")).unwrap(),
            fixture.bytes
        );
        if source {
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
                "Manual replacement\n"
            );
        } else {
            assert_ne!(
                fixture
                    .repository
                    .head()
                    .unwrap()
                    .target()
                    .unwrap()
                    .to_string(),
                fixture.head
            );
        }
        fixture.clean();
    }
}
