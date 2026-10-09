use super::*;
use sailry_protocol::plugin::Reference;

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

#[test]
fn removes_only_the_owned_session_directory() {
    let fixture = tempfile::tempdir().unwrap();
    let profile = fixture.path().canonicalize().unwrap();
    let removed = sailry_protocol::SessionId::new();
    let retained = sailry_protocol::SessionId::new();
    for id in [removed, retained] {
        let path = crate::workspaces::session(&profile, id);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("task.py"), "saved task").unwrap();
    }
    std::fs::write(profile.join("database.fixture"), "primary data").unwrap();
    remove_session(&profile, removed).unwrap();
    remove_session(&profile, removed).unwrap();
    assert!(!crate::workspaces::session(&profile, removed).exists());
    assert_eq!(
        std::fs::read(crate::workspaces::session(&profile, retained).join("task.py")).unwrap(),
        b"saved task"
    );
    assert_eq!(
        std::fs::read(profile.join("database.fixture")).unwrap(),
        b"primary data"
    );
}

#[cfg(unix)]
#[test]
fn refuses_linked_session_parents() {
    let fixture = tempfile::tempdir().unwrap();
    let profile = fixture.path().join("profile");
    let outside = fixture.path().join("outside");
    let id = sailry_protocol::SessionId::new();
    std::fs::create_dir_all(profile.join("workspaces")).unwrap();
    std::fs::create_dir_all(outside.join(id.to_string())).unwrap();
    std::fs::write(outside.join(id.to_string()).join("keep.py"), "keep").unwrap();
    std::os::unix::fs::symlink(&outside, profile.join("workspaces/sessions")).unwrap();
    assert!(remove_session(&profile.canonicalize().unwrap(), id).is_err());
    assert_eq!(
        std::fs::read(outside.join(id.to_string()).join("keep.py")).unwrap(),
        b"keep"
    );
}

#[cfg(unix)]
#[path = "tests/bundled.rs"]
mod bundled;
