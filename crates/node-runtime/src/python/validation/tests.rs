use super::*;

struct Fixture {
    _directory: tempfile::TempDir,
    base: PathBuf,
    path: PathBuf,
    key: String,
    cancel: CancellationToken,
    validation: Validation,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let base = root.join("runtime");
        let path = root.join("environment");
        let python = runtime::python(&path);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(python, "interpreter").unwrap();
        std::fs::write(path.join("package.py"), "original").unwrap();
        let key = "a".repeat(64);
        let cancel = CancellationToken::new();
        integrity::seal(&path, &base, &key, &cancel).unwrap();
        let validation = Validation::new(path.clone());
        Self {
            _directory: directory,
            base,
            path,
            key,
            cancel,
            validation,
        }
    }

    fn valid(&mut self) -> bool {
        self.validation
            .environment(&self.base, &self.key, &self.cancel)
            .unwrap()
    }
}

#[test]
fn reuses_verified_manifest() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    assert!(fixture.validation.backend.active());
    let checked = fixture.validation.verified.as_ref().unwrap().checked;
    assert!(fixture.valid());
    assert_eq!(
        fixture.validation.verified.as_ref().unwrap().checked,
        checked
    );
}

#[tokio::test]
async fn invalidates_on_nested_changes() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    let mut changes = fixture.validation.subscribe();
    std::fs::write(fixture.path.join("package.py"), "modified").unwrap();
    tokio::time::timeout(Duration::from_secs(15), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(!fixture.valid());
}

#[test]
fn expires_without_waiting_for_events() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    fixture.validation.verified.as_mut().unwrap().checked = Instant::now() - INTERVAL;
    let checked = fixture.validation.verified.as_ref().unwrap().checked;
    assert!(fixture.valid());
    assert!(fixture.validation.verified.as_ref().unwrap().checked > checked);
    std::fs::write(fixture.path.join("package.py"), "modified").unwrap();
    fixture.validation.verified.as_mut().unwrap().checked = Instant::now() - INTERVAL;
    assert!(!fixture.valid());
}

#[test]
fn detects_receipt_and_interpreter_replacement() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    std::fs::write(fixture.path.join(integrity::MARKER), b"{}").unwrap();
    assert!(!fixture.valid());
    integrity::seal(&fixture.path, &fixture.base, &fixture.key, &fixture.cancel).unwrap();
    assert!(fixture.valid());
    std::fs::write(runtime::python(&fixture.path), "replacement").unwrap();
    assert!(!fixture.valid());
}

#[test]
fn invalidates_removed_root() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    std::fs::remove_dir_all(&fixture.path).unwrap();
    assert!(!fixture.valid());
}

#[cfg(unix)]
#[test]
fn rejects_linked_root_on_hot_access() {
    let mut fixture = Fixture::new();
    assert!(fixture.valid());
    let replaced = fixture.path.with_file_name("replaced");
    std::fs::rename(&fixture.path, &replaced).unwrap();
    std::os::unix::fs::symlink(&replaced, &fixture.path).unwrap();
    assert!(!fixture.valid());
}

#[test]
fn validates_without_watch_capacity() {
    let mut fixture = Fixture::new();
    fixture.validation.backend = Backend::Limited(watchers::Watch::isolated(1, 1));
    assert!(fixture.valid());
    assert!(!fixture.validation.backend.active());
    assert!(fixture.validation.verified.is_none());
    assert!(fixture.valid());
    std::fs::write(fixture.path.join("package.py"), "modified").unwrap();
    assert!(!fixture.valid());
}

#[test]
fn revalidates_evicted_entries() {
    let watch = watchers::Watch::isolated(16, 1);
    let mut first = Fixture::new();
    let mut second = Fixture::new();
    first.validation.backend = Backend::Limited(watch.peer());
    second.validation.backend = Backend::Limited(watch);
    assert!(first.valid());
    let epoch = first.validation.verified.as_ref().unwrap().epoch;
    assert!(second.valid());
    assert!(!first.validation.backend.active());
    std::fs::write(first.path.join("package.py"), "modified").unwrap();
    first.validation.epoch.store(epoch, Ordering::Release);
    assert!(!first.valid());
}

#[tokio::test]
async fn watches_new_nested_directories() {
    let mut fixture = Fixture::new();
    fixture.validation.backend = Backend::Limited(watchers::Watch::isolated(16, 2));
    assert!(fixture.valid());
    let nested = fixture.path.join("lib/package");
    std::fs::create_dir_all(&nested).unwrap();
    let file = nested.join("module.py");
    std::fs::write(&file, "original").unwrap();
    integrity::seal(&fixture.path, &fixture.base, &fixture.key, &fixture.cancel).unwrap();
    assert!(fixture.valid());
    let mut changes = fixture.validation.subscribe();
    std::fs::write(file, "modified").unwrap();
    tokio::time::timeout(Duration::from_secs(15), changes.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(!fixture.valid());
}

#[test]
fn discards_incomplete_watch_coverage() {
    let mut fixture = Fixture::new();
    fixture.validation.backend = Backend::Limited(watchers::Watch::isolated(16, 2));
    let directory = fixture.path.join("lib");
    std::fs::create_dir(&directory).unwrap();
    let paths = [
        fixture.path.join(integrity::MARKER),
        runtime::python(&fixture.path),
    ];
    assert!(
        fixture
            .validation
            .validate(&paths, &fixture.cancel, |_, _| {
                std::fs::create_dir(directory.join("new")).unwrap();
                Ok(true)
            })
            .unwrap()
    );
    assert!(fixture.validation.verified.is_none());
    assert!(fixture.valid());
    assert!(fixture.validation.verified.is_some());
}
