use super::*;
use sailry_protocol::process::Outcome;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("/usr/bin/git")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn run(
    root: &Path,
    profile: &Path,
    home: &Path,
    command: String,
) -> sailry_protocol::process::Completion {
    // Node registration supplies a canonical root. Preserve no-follow checks
    // when a platform's temporary-directory alias is used by this fixture.
    let root = root.canonicalize().unwrap();
    let profile = profile.canonicalize().unwrap();
    let mut request = launch(&root, &profile);
    request.command = command;
    request.timeout_ms = 10_000;
    crate::process::execute(
        request,
        &environment(home).await,
        &sailry_link::CancellationToken::new(),
        &sailry_link::CancellationToken::new(),
    )
    .await
    .unwrap()
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[tokio::test]
async fn preserves_metadata_and_workspace_edits() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let home = directory.path().join("home");
    let profile = directory.path().join("profile");
    for path in [&root, &home, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    git(&root, &["init", "--quiet"]);
    std::fs::write(root.join("tracked.txt"), "original").unwrap();
    git(&root, &["add", "tracked.txt"]);
    git(&root, &["commit", "--quiet", "-m", "Fixture"]);
    std::fs::write(root.join(".git/hooks/pre-commit"), "original hook").unwrap();
    let config = std::fs::read(root.join(".git/config")).unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let output = run(
        &root,
        &profile,
        &home,
        String::from(
            "git --version > version.txt || exit 90; git status --short > status.txt || exit 91; \
         printf changed > tracked.txt || exit 92; \
         if printf changed > .git/config; then exit 93; fi; \
         if printf changed > .git/hooks/pre-commit; then exit 94; fi; \
         if printf changed > .git/index; then exit 95; fi; \
         if mv .git renamed-metadata; then exit 96; fi; printf protected",
        ),
    )
    .await;
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(output.stdout.text, "protected");
    assert!(
        std::fs::read_to_string(root.join("version.txt"))
            .unwrap()
            .starts_with("git version ")
    );
    assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), config);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(root.join(".git/hooks/pre-commit")).unwrap(),
        "original hook"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "changed"
    );
    assert!(!root.join("renamed-metadata").exists());
}

#[tokio::test]
async fn captures_nested_and_linked_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let repository = directory.path().join("repository");
    let home = directory.path().join("home");
    let profile = directory.path().join("profile");
    for path in [&root, &repository, &home, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    git(&repository, &["init", "--quiet"]);
    std::fs::write(repository.join("tracked.txt"), "original").unwrap();
    git(&repository, &["add", "tracked.txt"]);
    git(&repository, &["commit", "--quiet", "-m", "Fixture"]);
    let linked = root.join("linked");
    git(
        &repository,
        &["worktree", "add", "--quiet", linked.to_str().unwrap()],
    );
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "--quiet"]);
    let ignored = root.join("ignored");
    std::fs::create_dir(&ignored).unwrap();
    git(&ignored, &["init", "--quiet"]);
    std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    std::os::unix::fs::symlink(&repository, root.join("alias")).unwrap();
    let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment(&home).await, None)
        .unwrap()
        .unwrap();
    let protected = &sandbox.protected;
    for path in [
        &nested.join(".git"),
        &linked.join(".git"),
        &repository.join(".git"),
        &repository.join(".git/worktrees/linked"),
    ] {
        assert!(
            protected.contains(&path.canonicalize().unwrap()),
            "{protected:?}"
        );
    }
    assert!(!protected.contains(&ignored.join(".git").canonicalize().unwrap()));
    assert!(
        !protected
            .iter()
            .any(|path| path.starts_with(root.join("alias")))
    );
    let pointer = std::fs::read(linked.join(".git")).unwrap();
    let common = repository.join(".git");
    let output = run(
        &root,
        &profile,
        &home,
        format!(
            "git -C linked status --short > linked-status.txt || exit 90; \
         if printf changed > linked/.git; then exit 91; fi; \
         if printf changed > nested/.git/config; then exit 92; fi; \
         if printf changed > {}/config; then exit 93; fi; \
         printf allowed > linked/tracked.txt || exit 94; printf protected",
            quote(&common)
        ),
    )
    .await;
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(output.stdout.text, "protected");
    assert_eq!(std::fs::read(linked.join(".git")).unwrap(), pointer);
    assert_eq!(
        std::fs::read_to_string(linked.join("tracked.txt")).unwrap(),
        "allowed"
    );
}

#[tokio::test]
async fn protects_ancestor_and_symlink_targets() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository");
    let home = directory.path().join("home");
    let profile = directory.path().join("profile");
    for path in [&repository, &home, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    git(&repository, &["init", "--quiet"]);
    let root = repository.join("subdirectory");
    std::fs::create_dir(&root).unwrap();
    let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment(&home).await, None)
        .unwrap()
        .unwrap();
    assert!(
        sandbox
            .protected
            .contains(&repository.join(".git").canonicalize().unwrap())
    );
    let alias = root.join("alias");
    std::fs::create_dir(&alias).unwrap();
    std::os::unix::fs::symlink(repository.join(".git"), alias.join(".git")).unwrap();
    let sandbox = Sandbox::prepare(&launch(&root, &profile), &environment(&home).await, None)
        .unwrap()
        .unwrap();
    assert!(
        sandbox
            .protected
            .contains(&alias.canonicalize().unwrap().join(".git"))
    );
    let config = std::fs::read(repository.join(".git/config")).unwrap();
    let output = run(
        &root,
        &profile,
        &home,
        String::from("if printf changed > alias/.git/config; then exit 90; fi; printf protected"),
    )
    .await;
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(
        std::fs::read(repository.join(".git/config")).unwrap(),
        config
    );
}

#[tokio::test]
async fn protects_native_metadata_despite_unreadable_siblings() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let home = directory.path().join("home");
    let profile = directory.path().join("profile");
    for path in [&root, &home, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    let native = root.join(if cfg!(target_os = "linux") {
        std::ffi::OsString::from_vec(b"native-\xff".to_vec())
    } else {
        std::ffi::OsString::from("native-资料")
    });
    std::fs::create_dir(&native).unwrap();
    git(&native, &["init", "--quiet"]);
    let original = std::fs::read(native.join(".git/config")).unwrap();
    let restricted = root.join("restricted");
    std::fs::create_dir(&restricted).unwrap();
    std::fs::set_permissions(&restricted, std::fs::Permissions::from_mode(0)).unwrap();
    let output = run(
        &root,
        &profile,
        &home,
        String::from(
            "if printf changed > native-*/.git/config; then exit 90; fi; \
             printf allowed > output.txt || exit 91; printf protected",
        ),
    )
    .await;
    std::fs::set_permissions(&restricted, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(output.stdout.text, "protected");
    assert_eq!(std::fs::read(native.join(".git/config")).unwrap(), original);
    assert_eq!(std::fs::read(root.join("output.txt")).unwrap(), b"allowed");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn rejects_future_case_insensitive_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("work");
    let home = directory.path().join("home");
    let profile = directory.path().join("profile");
    for path in [&root, &home, &profile] {
        std::fs::create_dir(path).unwrap();
    }
    let output = run(
        &root,
        &profile,
        &home,
        String::from(
            "mkdir nested || exit 90; \
         if mkdir nested/.git; then exit 91; fi; \
         if printf changed > nested/.GiT; then exit 92; fi; \
         printf allowed > nested/file.txt || exit 93; printf protected",
        ),
    )
    .await;
    assert_eq!(output.outcome, Outcome::Exited(0), "{output:?}");
    assert_eq!(output.stdout.text, "protected");
    assert!(!root.join("nested/.git").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("nested/file.txt")).unwrap(),
        "allowed"
    );
}
