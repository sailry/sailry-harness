#![cfg(unix)]
use super::*;
use crate::code::{
    fixtures::CodeServerFixture,
    tests::{Fixture, network::Sources},
};
use serde_json::json;
use sha2::{Digest, Sha256};

async fn fixture(hold: bool, checksum: bool) -> (Fixture, Arc<Manager>, Sources, code::Install) {
    let fixture = Fixture::new().await;
    let description = CodeServerFixture::new();
    let bytes = CodeServerFixture::archive(CodeServerFixture::python());
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let filename = description.asset_url.rsplit('/').next().unwrap();
    let digest = if checksum { hash } else { "0".repeat(64) };
    let routes = BTreeMap::from([
        (description.asset_path(), bytes),
        (
            description.release_path(),
            serde_json::to_vec(
                &json!({"assets":[{"name":filename,"digest":format!("sha256:{digest}")}]}),
            )
            .unwrap(),
        ),
    ]);
    let sources = Sources::new(routes, hold.then(|| description.asset_path())).await;
    let network = Arc::new(Network::default());
    network.fixture(&sources.endpoint);
    let rules = Arc::new(Registry::new(
        Some(fixture._directory.path().join("profile")),
        network.clone(),
    ));
    let manager = Arc::new(Manager::new(
        Some(fixture._directory.path().join("profile")),
        fixture.environment.clone(),
        rules.clone(),
        network,
    ));
    let package = rules
        .get()
        .unwrap()
        .packages
        .iter()
        .find(|package| package.name == "rust-analyzer")
        .unwrap()
        .clone();
    let install = plan::package(&package, &fixture.environment)
        .install
        .unwrap();
    (fixture, manager, sources, install)
}

#[tokio::test]
async fn publishes_verified_receipts_and_removes_exact_versions() {
    let (_fixture, manager, sources, install) = fixture(false, true).await;
    let phases = Arc::new(Mutex::new(Vec::new()));
    let captured = phases.clone();
    let reporter: Reporter = Arc::new(move |phase, progress| {
        assert!(progress.is_none());
        captured.lock().unwrap().push(phase);
    });
    let state = manager
        .install(&install, &CancellationToken::new(), Some(&reporter))
        .await
        .unwrap();
    assert_eq!(state.status, code::InstallStatus::Ready);
    let phases = phases.lock().unwrap().clone();
    assert_eq!(phases.first(), Some(&Phase::Preparing));
    assert!(phases.contains(&Phase::Downloading));
    assert!(phases.contains(&Phase::Validating));
    assert_eq!(phases.last(), Some(&Phase::Installing));
    let receipt = state.receipt.unwrap();
    assert!(receipt.downloads[0].verified);
    assert_eq!(receipt.revision, install.revision);
    let directory = manager.path(&install.package, &install.version).unwrap();
    assert!(directory.join("bin/rust-analyzer").is_file());
    assert_eq!(manager.list().unwrap()[0].receipt.as_ref(), Some(&receipt));
    let count = sources.requests.lock().await.len();
    assert!(
        manager
            .install(&install, &CancellationToken::new(), None)
            .await
            .is_ok()
    );
    assert_eq!(sources.requests.lock().await.len(), count);
    assert!(manager.uninstall("../other", &install.version).is_err());
    manager
        .uninstall(&install.package, &install.version)
        .unwrap();
    assert!(!directory.exists());
    assert!(manager.list().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_cleans_staging_and_preserves_other_packages() {
    let (_fixture, manager, sources, install) = fixture(true, true).await;
    let running = manager.clone();
    let requested = install.clone();
    let task = tokio::spawn(async move {
        running
            .install(&requested, &CancellationToken::new(), None)
            .await
    });
    sources
        .requested(&CodeServerFixture::new().asset_path())
        .await;
    assert_eq!(
        manager
            .validate_uninstall(&install.package, &install.version)
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert!(matches!(
        manager
            .install(&install, &CancellationToken::new(), None)
            .await,
        Err(Fault {
            code: ErrorCode::Busy,
            ..
        })
    ));
    assert!(manager.cancel("other").is_err());
    manager.cancel(&install.package).unwrap();
    assert_eq!(task.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
    let parent = manager
        .path(&install.package, &install.version)
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
    assert_eq!(
        manager.list().unwrap()[0].status,
        code::InstallStatus::Cancelled
    );
    sources.release.notify_waiters();
}

#[tokio::test]
async fn checksum_failure_leaves_no_installed_version() {
    let (_fixture, manager, _sources, install) = fixture(false, false).await;
    assert!(
        manager
            .install(&install, &CancellationToken::new(), None)
            .await
            .is_err()
    );
    let path = manager.path(&install.package, &install.version).unwrap();
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        0
    );
    assert_eq!(
        manager.list().unwrap()[0].status,
        code::InstallStatus::Failed
    );
}

mod selection {
    use super::*;
    use crate::code::{
        resolve,
        rules::{Package, Rules, Server},
    };
    use std::os::unix::fs::PermissionsExt;

    fn record(profile: &Path, package: &Package, version: &str, timestamp: &str) -> code::Receipt {
        let directory = profile.join("tools/lsp").join(&package.name).join(version);
        std::fs::create_dir_all(directory.join("bin")).unwrap();
        let executable = directory.join("bin/ruff");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = package.source["id"]
            .as_str()
            .unwrap()
            .rsplit_once('@')
            .unwrap()
            .0;
        let receipt = code::Receipt {
            package: package.name.clone(),
            version: version.into(),
            purl: format!("{source}@{version}"),
            revision: "a".repeat(64),
            downloads: Vec::new(),
            installed_at: timestamp.into(),
        };
        std::fs::write(
            directory.join("receipt.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        receipt
    }

    #[tokio::test]
    async fn prefers_current_versions_and_validated_receipt_dates() {
        let fixture = Fixture::new().await;
        let profile = fixture._directory.path().join("profile");
        let original = Rules::load().unwrap();
        let package = original
            .packages
            .iter()
            .find(|package| package.name == "ruff")
            .unwrap();
        assert_eq!(version(package).unwrap(), "0.16.10");
        let old = record(&profile, package, "0.9.10", "2026-10-09T00:00:00Z");
        let current = record(&profile, package, "0.16.10", "2026-10-08T00:00:00Z");
        let recent = record(&profile, package, "0.10.0", "2026-10-10T00:00:00Z");
        let invalid = record(&profile, package, "0.15.0", "not a timestamp");
        let network = Arc::new(Network::default());
        let rules = Arc::new(Registry::new(Some(profile.clone()), network.clone()));
        let manager = Manager::new(
            Some(profile.clone()),
            fixture.environment.clone(),
            rules.clone(),
            network.clone(),
        );
        manager.jobs.lock().unwrap().insert(
            "ruff".into(),
            Job {
                stop: CancellationToken::new(),
                state: code::Installation {
                    package: "ruff".into(),
                    status: code::InstallStatus::Ready,
                    progress: None,
                    receipt: Some(old.clone()),
                    error: None,
                },
            },
        );
        let server = Server {
            command: "ruff".into(),
            args: Vec::new(),
            config: Value::Null,
            environment: BTreeMap::new(),
            timeout: 20,
            roots: Vec::new(),
        };
        let selected = || {
            resolve::executable(
                &fixture.root,
                &fixture.root,
                Some(&profile),
                &fixture.environment,
                &server,
                Some(package),
            )
            .unwrap()
        };
        assert_eq!(
            selected().path,
            profile
                .join("tools/lsp/ruff/0.16.10/bin/ruff")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(
            selected().dependency,
            profile
                .join("tools/lsp/ruff/0.16.10")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(manager.list().unwrap()[0].receipt.as_ref(), Some(&current));
        let restarted = Manager::new(
            Some(profile.clone()),
            fixture.environment.clone(),
            rules,
            network,
        );
        assert_eq!(restarted.list().unwrap(), manager.list().unwrap());
        std::fs::remove_dir_all(profile.join("tools/lsp/ruff/0.16.10")).unwrap();
        assert_eq!(
            selected().path,
            profile
                .join("tools/lsp/ruff/0.10.0/bin/ruff")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(
            selected().dependency,
            profile
                .join("tools/lsp/ruff/0.10.0")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(manager.list().unwrap()[0].receipt.as_ref(), Some(&recent));
        assert_eq!(restarted.list().unwrap(), manager.list().unwrap());
        std::fs::remove_dir_all(profile.join("tools/lsp/ruff/0.10.0")).unwrap();
        assert_eq!(
            selected().path,
            profile
                .join("tools/lsp/ruff/0.9.10/bin/ruff")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(manager.list().unwrap()[0].receipt.as_ref(), Some(&old));
        assert!(
            profile
                .join("tools/lsp/ruff")
                .join(&invalid.version)
                .exists()
        );
    }
}
