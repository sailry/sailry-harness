use super::*;
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct Denied {
    path: PathBuf,
    permissions: std::fs::Permissions,
}

impl Denied {
    fn new(path: &Path) -> Option<Self> {
        Self::with_mode(path, 0o000)
    }

    fn with_mode(path: &Path, mode: u32) -> Option<Self> {
        let denied = Self {
            path: path.to_owned(),
            permissions: std::fs::metadata(path).unwrap().permissions(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        // Privileged runners can bypass mode bits and cannot exercise this fixture.
        if std::fs::read_dir(path).is_ok() {
            return None;
        }
        Some(denied)
    }
}

impl Drop for Denied {
    fn drop(&mut self) {
        std::fs::set_permissions(&self.path, self.permissions.clone())
            .expect("restore fixture permissions");
    }
}

fn control() -> Control {
    Control {
        cancelled: sailry_link::CancellationToken::new(),
        deadline: std::time::Instant::now() + Duration::from_secs(5),
    }
}

#[test]
fn skips_only_the_inaccessible_subtree() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = Repository::init(&root).unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    std::fs::create_dir(root.join("locked2")).unwrap();
    std::fs::write(root.join("locked/tracked.txt"), "hidden\n").unwrap();
    std::fs::write(root.join("locked2/sibling.txt"), "sibling\n").unwrap();
    std::fs::write(root.join("visible.txt"), "visible\n").unwrap();
    let Some(_denied) = Denied::new(&root.join("locked")) else {
        return;
    };
    let mut scan = Candidates {
        paths: [
            "locked",
            "locked/tracked.txt",
            "locked2/sibling.txt",
            "visible.txt",
        ]
        .map(str::to_owned)
        .into(),
        scanned_entries: 0,
        omitted_paths: 0,
        budget: 100,
        truncated: false,
    };
    scan_workdir(
        &path::root(&root).unwrap(),
        &repository,
        &mut scan,
        &control(),
    )
    .unwrap();
    assert_eq!(
        scan.paths,
        ["locked2/sibling.txt", "visible.txt"]
            .map(str::to_owned)
            .into()
    );
    assert!(!scan.truncated);
    assert_eq!(scan.omitted_paths, 0);
}

#[test]
fn filters_ignored_tracked_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let repository = Repository::init(&root).unwrap();
    std::fs::create_dir(root.join("locked")).unwrap();
    std::fs::create_dir(root.join("locked2")).unwrap();
    std::fs::write(root.join("locked/tracked.txt"), "hidden\n").unwrap();
    std::fs::write(root.join("locked2/sibling.txt"), "sibling\n").unwrap();
    let mut index = repository.index().unwrap();
    for path in ["locked/tracked.txt", "locked2/sibling.txt"] {
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    std::fs::write(root.join(".gitignore"), "locked/\n").unwrap();
    assert!(
        repository
            .status_should_ignore(Path::new("locked/"))
            .unwrap()
    );
    let Some(_denied) = Denied::new(&root.join("locked")) else {
        return;
    };
    let mut scan = Candidates {
        paths: BTreeSet::new(),
        scanned_entries: 0,
        omitted_paths: 0,
        budget: 100,
        truncated: false,
    };
    scan_index(&repository, &mut scan, &control()).unwrap();
    scan_workdir(
        &path::root(&root).unwrap(),
        &repository,
        &mut scan,
        &control(),
    )
    .unwrap();
    assert_eq!(
        scan.paths,
        [".gitignore", "locked2/sibling.txt"]
            .map(str::to_owned)
            .into()
    );
    assert!(!scan.truncated);
    assert_eq!(scan.omitted_paths, 0);
}

#[test]
fn filters_after_the_budget_is_exhausted() {
    for mode in [0o000, 0o111] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let repository = Repository::init(&root).unwrap();
        std::fs::create_dir_all(root.join("locked/deep")).unwrap();
        std::fs::create_dir(root.join("locked2")).unwrap();
        std::fs::create_dir(root.join("missing")).unwrap();
        let paths = [
            "locked/deep/tracked.txt",
            "locked/tracked.txt",
            "locked2/sibling.txt",
            "missing/tracked.txt",
            "visible.txt",
        ];
        let mut index = repository.index().unwrap();
        for value in paths {
            std::fs::write(root.join(value), "tracked\n").unwrap();
            index.add_path(Path::new(value)).unwrap();
        }
        index.write().unwrap();
        std::fs::remove_file(root.join("missing/tracked.txt")).unwrap();
        std::fs::remove_dir(root.join("missing")).unwrap();
        let Some(_denied) = Denied::with_mode(&root.join("locked"), mode) else {
            return;
        };
        if mode == 0o111 {
            assert!(std::fs::symlink_metadata(root.join("locked/tracked.txt")).is_ok());
        }
        let mut scan = Candidates {
            paths: BTreeSet::new(),
            scanned_entries: 0,
            omitted_paths: 0,
            budget: paths.len(),
            truncated: false,
        };
        scan_index(&repository, &mut scan, &control()).unwrap();
        assert!(!scan.has_budget());
        scan_workdir(
            &path::root(&root).unwrap(),
            &repository,
            &mut scan,
            &control(),
        )
        .unwrap();
        assert_eq!(
            scan.paths,
            ["locked2/sibling.txt", "missing/tracked.txt", "visible.txt"]
                .map(str::to_owned)
                .into()
        );
        assert!(scan.truncated);
        assert_eq!(scan.scanned_entries, paths.len());
        assert_eq!(scan.omitted_paths, 0);
    }
}

#[test]
fn preserves_missing_and_replaced_ancestors() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repository");
    let repository = Repository::init(&root).unwrap();
    std::fs::write(root.join("file"), "directory replacement\n").unwrap();
    std::fs::create_dir(directory.path().join("outside")).unwrap();
    std::os::unix::fs::symlink(directory.path().join("outside"), root.join("link")).unwrap();
    let paths: BTreeSet<_> = ["file/old.txt", "link/old.txt", "missing/old.txt"]
        .map(str::to_owned)
        .into();
    let mut scan = Candidates {
        paths: paths.clone(),
        scanned_entries: 0,
        omitted_paths: 0,
        budget: 0,
        truncated: false,
    };
    scan_workdir(
        &path::root(&root.canonicalize().unwrap()).unwrap(),
        &repository,
        &mut scan,
        &control(),
    )
    .unwrap();
    assert_eq!(scan.paths, paths);
    assert_eq!(scan.omitted_paths, 0);
    assert!(scan.truncated);
}

#[test]
fn rejects_an_inaccessible_root() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    Repository::init(&root).unwrap();
    let Some(_denied) = Denied::new(&root) else {
        return;
    };
    assert_eq!(
        inspect(&root, &control()).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn rejects_inaccessible_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    Repository::init(&root).unwrap();
    let Some(_denied) = Denied::new(&root.join(".git")) else {
        return;
    };
    assert!(inspect(&root, &control()).is_err());
}
