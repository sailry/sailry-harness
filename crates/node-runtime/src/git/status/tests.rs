use super::*;
use std::{os::unix::fs::PermissionsExt, time::Duration};

struct Denied {
    path: PathBuf,
    permissions: std::fs::Permissions,
}

impl Denied {
    fn new(path: &Path) -> Option<Self> {
        let denied = Self {
            path: path.to_owned(),
            permissions: std::fs::metadata(path).unwrap().permissions(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
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
