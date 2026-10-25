use super::*;
#[cfg(unix)]
use crate::code::{Pool, tests::Fixture};
#[cfg(unix)]
use sailry_link::CancellationToken;

#[test]
fn tracks_components_from_captured_home() {
    for key in ["HOME", "USERPROFILE"] {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        let profile = directory.path().join("profile");
        let home = profile.join(".rustup");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let launcher = bin.join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
        let executable = bin.join(format!("rust-analyzer{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&launcher, "fixture rustup launcher").unwrap();
        std::fs::hard_link(&launcher, &executable).unwrap();
        std::fs::write(
            home.join("settings.toml"),
            "default_toolchain = 'stable-fixture-host'\n",
        )
        .unwrap();
        let environment =
            crate::process::Environment::filtered([(key.into(), profile.as_os_str().to_owned())]);
        let variables = BTreeMap::new();
        assert!(!available(&executable, None, &environment, &[], &variables));

        let toolchain = home.join("toolchains/stable-fixture-host/bin");
        std::fs::create_dir_all(&toolchain).unwrap();
        let installed = toolchain.join(executable.file_name().unwrap());
        std::fs::write(&installed, "fixture language server").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(available(&executable, None, &environment, &[], &variables));
    }
}

#[cfg(unix)]
fn proxy(fixture: &Fixture, hard_link: bool) -> PathBuf {
    let executable = fixture.bin.join("rust-analyzer");
    std::fs::remove_file(&executable).unwrap();
    let launcher = fixture.bin.join("rustup");
    std::fs::write(&launcher, "#!/bin/sh\nexit 92\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    if hard_link {
        std::fs::hard_link(&launcher, &executable).unwrap();
    } else {
        std::os::unix::fs::symlink(&launcher, &executable).unwrap();
    }
    let home = fixture.environment.home().unwrap().join(".rustup");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("settings.toml"),
        "default_toolchain = 'stable-x86_64-unknown-linux-gnu'\n",
    )
    .unwrap();
    home
}

#[cfg(unix)]
fn component(home: &Path, name: &str) {
    let bin = home.join("toolchains").join(name).join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("rust-analyzer");
    std::fs::write(&executable, "#!/bin/sh\nexit 93\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn missing_components_are_not_installed() {
    for hard_link in [false, true] {
        let fixture = Fixture::new().await;
        let home = proxy(&fixture, hard_link);
        let resolved = super::super::resolve(
            fixture.worktree,
            &fixture.root,
            None,
            &fixture.environment,
            "main.rs",
        )
        .unwrap();
        assert!(resolved.plan.is_none());
        assert_eq!(resolved.package.as_deref(), Some("rust-analyzer"));
        let pool = Pool::new(None, fixture.environment.clone(), CancellationToken::new());
        let catalog = pool.catalog().await.unwrap();
        let candidate = catalog
            .iter()
            .flat_map(|language| &language.servers)
            .find(|candidate| candidate.server == "rust-analyzer")
            .unwrap();
        assert_eq!(
            candidate.status,
            sailry_protocol::code::Status::NotInstalled
        );
        assert!(candidate.package.as_ref().unwrap().install.is_some());
        component(&home, "stable-x86_64-unknown-linux-gnu");
        assert!(fixture.plan().startup.executable.ends_with("rust-analyzer"));
        assert!(!fixture.root.join("pid").exists());
        pool.shutdown().await;
    }
}

#[tokio::test]
#[cfg(unix)]
async fn keeps_explicit_dispatchers_available() {
    let fixture = Fixture::new().await;
    proxy(&fixture, false);
    let executable = fixture.bin.join("rustup");
    assert!(available(
        &executable,
        Some(&fixture.root),
        &fixture.environment,
        &["run".into(), "stable".into(), "rust-analyzer".into()],
        &BTreeMap::new(),
    ));
    assert!(!fixture.root.join("pid").exists());
}

#[tokio::test]
#[cfg(unix)]
async fn respects_project_and_explicit_overrides() {
    let fixture = Fixture::new().await;
    let home = proxy(&fixture, false);
    component(&home, "stable-x86_64-unknown-linux-gnu");
    std::fs::write(
        fixture.root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = '1.98.0'\nprofile = 'minimal'\n",
    )
    .unwrap();
    let executable = fixture.bin.join("rust-analyzer");
    assert!(!available(
        &executable,
        Some(&fixture.root),
        &fixture.environment,
        &[],
        &BTreeMap::new(),
    ));
    let variables = BTreeMap::from([(
        "RUSTUP_TOOLCHAIN".into(),
        "stable-x86_64-unknown-linux-gnu".into(),
    )]);
    assert!(available(
        &executable,
        Some(&fixture.root),
        &fixture.environment,
        &[],
        &variables,
    ));
    assert!(!available(
        &executable,
        Some(&fixture.root),
        &fixture.environment,
        &["+1.98.0".into()],
        &variables,
    ));
    component(&home, "1.98.0-x86_64-unknown-linux-gnu");
    assert!(available(
        &executable,
        Some(&fixture.root),
        &fixture.environment,
        &[],
        &BTreeMap::new(),
    ));
    assert!(!fixture.root.join("pid").exists());
}
