use super::*;
use sailry_protocol::plugin::Reference;
use std::sync::atomic::Ordering;

fn requirements(name: &str, content: &str) -> Requirements {
    Requirements::new(
        Reference {
            name: name.into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        content.into(),
    )
    .unwrap()
}

fn manager() -> (tempfile::TempDir, Manager) {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().canonicalize().unwrap();
    let root = profile.join("runtime");
    let relative = if cfg!(windows) {
        "python.exe"
    } else {
        "bin/python3.12"
    };
    let python = root.join(relative);
    std::fs::create_dir_all(python.parent().unwrap()).unwrap();
    std::fs::write(&python, b"fixture interpreter").unwrap();
    let marker = root.join(if cfg!(windows) {
        "Lib/EXTERNALLY-MANAGED"
    } else {
        "lib/python3.12/EXTERNALLY-MANAGED"
    });
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(marker, b"managed").unwrap();
    std::fs::write(
        root.join("python-runtime.json"),
        serde_json::to_vec(&serde_json::json!({
            "version":1,
            "python_version":VERSION,
            "target":"fixture",
            "source":{"url":"https://example.test/python.tar.gz","sha256":"a".repeat(64)},
            "files":[{"path":relative,"size":19,"sha256":integrity::hash(&python,&CancellationToken::new()).unwrap()}]
        }))
        .unwrap(),
    )
    .unwrap();
    let runtime = Runtime::open(&root).unwrap();
    let manager = Manager::new(profile, runtime, Arc::new(Environment::ambient())).unwrap();
    (directory, manager)
}

#[tokio::test]
async fn reuses_ready_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().canonicalize().unwrap();
    let requirements = requirements("fixture", "example==1.0 --hash=sha256:abcd\n");
    let environment = Arc::new(Environment::ambient());
    let progress = Arc::new(watch::channel(Vec::new()).0);
    let (manager, installations) = Manager::ready_fixture(
        profile.clone(),
        &requirements,
        environment.clone(),
        progress.clone(),
    )
    .unwrap();
    let key = requirements.key(&manager.runtime.identity);
    let root = manager.root.join("environments").join(key);
    let paths = [
        manager.runtime.python.clone(),
        manager.runtime.root.join("python-runtime.json"),
        runtime::python(&root),
        root.join(".sailry-requirements.txt"),
        root.join(integrity::MARKER),
    ];
    let snapshot = || {
        paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect::<Vec<_>>()
    };
    let before = snapshot();
    let stop = CancellationToken::new();
    let ready = manager.ensure(Some(&requirements), &stop).await.unwrap();
    assert_eq!(ready.environment, root);
    assert_eq!(installations.load(Ordering::Relaxed), 0);
    drop(ready);
    drop(manager);
    let (manager, installations) =
        Manager::ready_fixture(profile, &requirements, environment, progress).unwrap();
    let ready = manager.ensure(Some(&requirements), &stop).await.unwrap();
    assert_eq!(ready.environment, root);
    assert_eq!(installations.load(Ordering::Relaxed), 0);
    assert_eq!(snapshot(), before);
}

#[test]
fn rejects_corrupt_fixture_without_changes() {
    for base in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().canonicalize().unwrap();
        let requirements = requirements("fixture", "example==1.0 --hash=sha256:abcd\n");
        let environment = Arc::new(Environment::ambient());
        let progress = Arc::new(watch::channel(Vec::new()).0);
        let (manager, installations) = Manager::ready_fixture(
            profile.clone(),
            &requirements,
            environment.clone(),
            progress.clone(),
        )
        .unwrap();
        let key = requirements.key(&manager.runtime.identity);
        let root = manager.root.join("environments").join(key);
        let python = runtime::python(&root);
        let paths = [
            manager.runtime.python.clone(),
            manager.runtime.root.join("python-runtime.json"),
            python.clone(),
            root.join(".sailry-requirements.txt"),
            root.join(integrity::MARKER),
        ];
        std::fs::write(
            if base {
                &manager.runtime.python
            } else {
                &python
            },
            b"corrupted fixture interpreter",
        )
        .unwrap();
        let snapshot = || {
            paths
                .iter()
                .map(|path| std::fs::read(path).unwrap())
                .collect::<Vec<_>>()
        };
        let before = snapshot();
        drop(manager);
        assert!(Manager::ready_fixture(profile, &requirements, environment, progress).is_err());
        assert_eq!(snapshot(), before);
        assert_eq!(installations.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn reports_concurrent_preparations_without_paths() {
    let (_directory, manager) = manager();
    let first = requirements("first", "example==1.0 --hash=sha256:abcd\n");
    let second = requirements("second", "other==1.0 --hash=sha256:abcd\n");
    let _first = manager.request(&first);
    let _second = manager.request(&second);
    manager.report(&first, Stage::Creating);
    manager.report(&second, Stage::Installing);
    let snapshot = manager.preparations.borrow().clone();
    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().any(|item| item.package == "first"));
    assert!(snapshot.iter().any(|item| item.package == "second"));
    let json = serde_json::to_string(&snapshot).unwrap();
    assert!(!json.contains(manager.root.to_str().unwrap()));
    assert!(!json.contains(&first.package.digest));
    manager.report(&first, Stage::Ready);
    assert_eq!(manager.preparations.borrow().len(), 1);
    manager.report(&second, Stage::Failed);
    assert!(manager.preparations.borrow().is_empty());
}

#[test]
fn clears_progress_when_preparation_is_dropped() {
    let (_directory, manager) = manager();
    let requirements = requirements("fixture", "example==1.0 --hash=sha256:abcd\n");
    let requester = manager.request(&requirements);
    manager.report(&requirements, Stage::Creating);
    {
        let _preparing = Preparing {
            manager: &manager,
            requirements: &requirements,
        };
        assert_eq!(manager.preparations.borrow().len(), 1);
    }
    assert!(manager.preparations.borrow().is_empty());
    drop(requester);
    assert!(manager.active.lock().unwrap().is_empty());
}

#[test]
fn includes_waiters_sharing_one_environment() {
    let (_directory, manager) = manager();
    let first = requirements("first", "example==1.0 --hash=sha256:abcd\n");
    let second = requirements("second", &first.content);
    let first_request = manager.request(&first);
    manager.report(&first, Stage::Installing);
    let second_request = manager.request(&second);
    assert_eq!(manager.preparations.borrow().len(), 2);
    assert!(
        manager
            .preparations
            .borrow()
            .iter()
            .any(|item| item.package == "second")
    );
    drop(second_request);
    assert_eq!(manager.preparations.borrow().len(), 1);
    manager.finish(&first);
    drop(first_request);
    assert!(manager.preparations.borrow().is_empty());
    assert!(manager.active.lock().unwrap().is_empty());
}

#[test]
fn shares_identical_dependency_locks() {
    let first = requirements("first", "example==1.0 --hash=sha256:abcd\n");
    let second = requirements("second", &first.content);
    assert_eq!(first.key("runtime"), second.key("runtime"));
    assert_ne!(first.key("runtime"), first.key("updated-runtime"));
    assert_ne!(
        first.key("runtime"),
        requirements("first", "example==2.0 --hash=sha256:abcd\n").key("runtime")
    );
}

#[test]
fn rejects_installer_overrides_and_includes() {
    for content in [
        "-r other.txt",
        "--no-binary :all:",
        "--index-url https://example.test",
        "example @ file:///tmp/package.whl",
        "example==1.0 --trusted-host example.test",
    ] {
        assert!(
            Requirements::new(
                Reference {
                    name: "fixture".into(),
                    digest: "a".repeat(64),
                    settings_revision: 1
                },
                content.into()
            )
            .is_err(),
            "{content}"
        );
    }
    requirements(
        "fixture",
        "# generated lock\nexample==1.0 ; python_version >= '3.12' \\\n    --hash=sha256:abcd\n",
    );
}

#[test]
fn detects_corruption_and_missing_files() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let environment = root.join("environment");
    let runtime = root.join("runtime");
    std::fs::create_dir(&environment).unwrap();
    std::fs::create_dir(&runtime).unwrap();
    std::fs::write(environment.join("package.py"), "first").unwrap();
    let key = "a".repeat(64);
    let cancel = CancellationToken::new();
    integrity::seal(&environment, &runtime, &key, &cancel).unwrap();
    let mut cache = integrity::Cache::default();
    assert!(integrity::valid(&environment, &runtime, &key, &mut cache, &cancel).unwrap());
    std::fs::write(environment.join("package.py"), "other").unwrap();
    assert!(!integrity::valid(&environment, &runtime, &key, &mut cache, &cancel).unwrap());
    std::fs::write(environment.join("package.py"), "first").unwrap();
    assert!(integrity::valid(&environment, &runtime, &key, &mut cache, &cancel).unwrap());
    std::fs::remove_file(environment.join("package.py")).unwrap();
    assert!(!integrity::valid(&environment, &runtime, &key, &mut cache, &cancel).unwrap());
}

#[test]
fn verifies_bytecode_inventory() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let directory = root.join("__pycache__");
    std::fs::create_dir(&directory).unwrap();
    let cached = directory.join("package.cpython-312.pyc");
    std::fs::write(&cached, "locked bytecode").unwrap();
    let key = "a".repeat(64);
    let cancel = CancellationToken::new();
    integrity::seal(&root, &root, &key, &cancel).unwrap();
    let valid = || {
        integrity::valid(
            &root,
            &root,
            &key,
            &mut integrity::Cache::default(),
            &cancel,
        )
        .unwrap()
    };
    assert!(valid());
    std::fs::write(&cached, "changed bytecode").unwrap();
    assert!(!valid());
    std::fs::write(&cached, "locked bytecode").unwrap();
    assert!(valid());
    let added = directory.join("unlisted.cpython-312.pyc");
    std::fs::write(&added, "unlisted bytecode").unwrap();
    assert!(!valid());
    std::fs::remove_file(added).unwrap();
    assert!(valid());
}

#[cfg(unix)]
#[test]
fn confines_links_to_managed_environments() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().canonicalize().unwrap();
    let environment = root.join("environment");
    let runtime = root.join("runtime");
    std::fs::create_dir(&environment).unwrap();
    std::fs::create_dir(&runtime).unwrap();
    std::fs::write(runtime.join("python"), "interpreter").unwrap();
    std::os::unix::fs::symlink(runtime.join("python"), environment.join("python")).unwrap();
    let key = "a".repeat(64);
    let cancel = CancellationToken::new();
    integrity::seal(&environment, &runtime, &key, &cancel).unwrap();
    assert!(
        integrity::valid(
            &environment,
            &runtime,
            &key,
            &mut integrity::Cache::default(),
            &cancel
        )
        .unwrap()
    );
    std::fs::remove_file(environment.join("python")).unwrap();
    std::fs::write(root.join("outside"), "interpreter").unwrap();
    std::os::unix::fs::symlink(root.join("outside"), environment.join("python")).unwrap();
    assert!(
        !integrity::valid(
            &environment,
            &runtime,
            &key,
            &mut integrity::Cache::default(),
            &cancel
        )
        .unwrap()
    );
    assert!(integrity::seal(&environment, &runtime, &key, &cancel).is_err());
}

#[test]
fn preserves_cancellation_during_verification() {
    let fixture = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        integrity::valid(
            fixture.path(),
            fixture.path(),
            &"a".repeat(64),
            &mut integrity::Cache::default(),
            &cancel
        )
        .unwrap_err()
        .code,
        ErrorCode::Cancelled
    );
}

#[cfg(unix)]
#[path = "tests/bundled.rs"]
mod bundled;
